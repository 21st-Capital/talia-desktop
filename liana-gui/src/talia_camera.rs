//! Talia Desktop: reading QR codes with the computer's camera, for a vault
//! that comes from Talia on the iPhone (decided 2026-11-15).
//!
//! On the Mac, Apple's AVFoundation films and Apple's Vision reads the codes
//! in each picture, the reader the iPhone's camera uses: it copes with the
//! blur, glare and tilt of a phone held up to a webcam, where a plain reader
//! did not (decided 2026-11-15). Only text read from codes and a small
//! mirrored preview leave this module; no picture is ever stored. Elsewhere
//! there is no camera yet, and the app keeps its file import.

use iced::widget::image::Handle;

/// Whether the camera may be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Granted,
    /// Refused in System Settings, or not allowed on this Mac.
    Denied,
    NoCamera,
}

#[derive(Debug, Clone)]
pub enum Event {
    /// What the camera sees, for aiming, and the codes it read in it.
    Frame { preview: Handle, codes: Vec<String> },
    Failed(String),
}

#[cfg(target_os = "macos")]
pub use mac::{authorize, events, open_privacy_settings};

#[cfg(not(target_os = "macos"))]
pub async fn authorize() -> Access {
    Access::NoCamera
}

#[cfg(not(target_os = "macos"))]
pub fn events() -> impl iced::futures::Stream<Item = Event> {
    iced::futures::stream::empty()
}

#[cfg(not(target_os = "macos"))]
pub fn open_privacy_settings() {}

#[cfg(target_os = "macos")]
mod mac {
    use std::{
        sync::Mutex,
        time::{Duration, Instant},
    };

    use block2::RcBlock;
    use dispatch2::DispatchQueue;
    use iced::{futures::SinkExt, widget::image::Handle};
    use objc2::{
        define_class, msg_send,
        rc::{autoreleasepool, Retained},
        runtime::{AnyObject, Bool, ProtocolObject},
        AllocAnyThread, DefinedClass,
    };
    use objc2_av_foundation::{
        AVAuthorizationStatus, AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceInput,
        AVCaptureOutput, AVCaptureSession, AVCaptureSessionPreset1280x720,
        AVCaptureSessionPreset1920x1080, AVCaptureVideoDataOutput,
        AVCaptureVideoDataOutputSampleBufferDelegate, AVMediaTypeVideo,
    };
    use objc2_core_media::CMSampleBuffer;
    use objc2_core_video::{
        kCVPixelBufferPixelFormatTypeKey, kCVPixelFormatType_32BGRA, kCVReturnSuccess,
        CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
        CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth,
        CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    };
    use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSObject, NSObjectProtocol, NSString};
    use objc2_vision::{
        VNBarcodeSymbologyQR, VNDetectBarcodesRequest, VNImageOption, VNImageRequestHandler,
        VNRequest,
    };
    use tokio::sync::{mpsc, oneshot};

    use super::{Access, Event};

    /// A picture every this often is plenty to read a code changing every
    /// 300 ms, and keeps the computer cool.
    const EVERY: Duration = Duration::from_millis(90);
    /// The preview's width; the picture is read at full size.
    const PREVIEW_WIDTH: usize = 480;

    /// Asks macOS once; afterwards System Settings decides.
    pub async fn authorize() -> Access {
        let answer = {
            // SAFETY: AVFoundation's media type constant and class methods,
            // callable from any thread.
            let Some(video) = (unsafe { AVMediaTypeVideo }) else {
                return Access::NoCamera;
            };
            if unsafe { AVCaptureDevice::defaultDeviceWithMediaType(video) }.is_none() {
                return Access::NoCamera;
            }
            match unsafe { AVCaptureDevice::authorizationStatusForMediaType(video) } {
                AVAuthorizationStatus::Authorized => return Access::Granted,
                AVAuthorizationStatus::NotDetermined => {}
                _ => return Access::Denied,
            }
            let (sender, answer) = oneshot::channel();
            let sender = Mutex::new(Some(sender));
            let block = RcBlock::new(move |granted: Bool| {
                if let Some(sender) = sender.lock().ok().and_then(|mut s| s.take()) {
                    let _ = sender.send(granted.as_bool());
                }
            });
            // SAFETY: AVFoundation copies the block and calls it once.
            unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(video, &block) };
            answer
        };
        if answer.await.unwrap_or(false) {
            Access::Granted
        } else {
            Access::Denied
        }
    }

    /// The camera's pictures and the codes read in them, for as long as the
    /// stream is held: dropping it turns the camera off.
    pub fn events() -> impl iced::futures::Stream<Item = Event> {
        iced::stream::channel(2, async move |mut output| {
            let (sender, mut events) = mpsc::channel(2);
            std::thread::spawn(move || {
                if let Err(e) = film(&sender) {
                    let _ = sender.blocking_send(Event::Failed(e));
                }
            });
            while let Some(event) = events.recv().await {
                if output.send(event).await.is_err() {
                    break;
                }
            }
        })
    }

    /// The Mac's camera privacy settings, where a refusal is undone.
    pub fn open_privacy_settings() {
        let _ = open::that(
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Camera",
        );
    }

    /// Films until nobody listens any more.
    fn film(sender: &mpsc::Sender<Event>) -> Result<(), String> {
        // SAFETY: every AVFoundation object below is made, used and released
        // on this thread; the delegate runs on its own serial queue and only
        // reads the picture it is handed.
        unsafe {
            let video = AVMediaTypeVideo.ok_or("No camera")?;
            let device = AVCaptureDevice::defaultDeviceWithMediaType(video).ok_or("No camera")?;
            let input = AVCaptureDeviceInput::deviceInputWithDevice_error(&device)
                .map_err(|e| e.localizedDescription().to_string())?;
            let session = AVCaptureSession::new();
            // The most pixels a module, for a code held at arm's length.
            for preset in [AVCaptureSessionPreset1920x1080, AVCaptureSessionPreset1280x720] {
                if session.canSetSessionPreset(preset) {
                    session.setSessionPreset(preset);
                    break;
                }
            }
            if !session.canAddInput(&input) {
                return Err("The camera is busy".into());
            }
            session.addInput(&input);

            let output = AVCaptureVideoDataOutput::new();
            let key: &NSString = &*(kCVPixelBufferPixelFormatTypeKey as *const _ as *const NSString);
            let bgra = NSNumber::new_u32(kCVPixelFormatType_32BGRA);
            let settings = NSDictionary::<NSString, AnyObject>::from_slices(&[key], &[&*bgra]);
            output.setVideoSettings(Some(&settings));
            output.setAlwaysDiscardsLateVideoFrames(true);
            let delegate = Delegate::new(sender.clone());
            let queue = DispatchQueue::new("com.twentyfirstcapital.taliadesktop.camera", None);
            output.setSampleBufferDelegate_queue(
                Some(ProtocolObject::from_ref(&*delegate)),
                Some(&queue),
            );
            if !session.canAddOutput(&output) {
                return Err("The camera is busy".into());
            }
            session.addOutput(&output);

            session.startRunning();
            while !sender.is_closed() {
                std::thread::sleep(Duration::from_millis(150));
            }
            session.stopRunning();
            output.setSampleBufferDelegate_queue(None, None);
        }
        Ok(())
    }

    struct Ivars {
        sender: mpsc::Sender<Event>,
        last: Mutex<Option<Instant>>,
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements and the delegate
        // does not implement Drop.
        #[unsafe(super(NSObject))]
        #[name = "TaliaDesktopQrCameraDelegate"]
        #[ivars = Ivars]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for Delegate {
            #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
            fn did_output_sample_buffer(
                &self,
                _output: &AVCaptureOutput,
                sample_buffer: &CMSampleBuffer,
                _connection: &AVCaptureConnection,
            ) {
                self.picture(sample_buffer);
            }
        }
    );

    impl Delegate {
        fn new(sender: mpsc::Sender<Event>) -> Retained<Self> {
            let this = Self::alloc().set_ivars(Ivars {
                sender,
                last: Mutex::new(None),
            });
            // SAFETY: NSObject's init.
            unsafe { msg_send![super(this), init] }
        }

        fn picture(&self, sample_buffer: &CMSampleBuffer) {
            let ivars = self.ivars();
            {
                let Ok(mut last) = ivars.last.lock() else {
                    return;
                };
                if last.is_some_and(|at| at.elapsed() < EVERY) {
                    return;
                }
                *last = Some(Instant::now());
            }
            if ivars.sender.capacity() == 0 {
                return;
            }
            // Vision leaves its work to the pool: emptied after each picture.
            autoreleasepool(|_| {
                // SAFETY: the picture is only read, while AVFoundation holds it.
                let Some(pixels) = (unsafe { sample_buffer.image_buffer() }) else {
                    return;
                };
                let codes = read_codes(&pixels);
                let Some(preview) = (unsafe { preview(&pixels) }) else {
                    return;
                };
                let _ = ivars.sender.try_send(Event::Frame { preview, codes });
            });
        }
    }

    /// The QR codes Apple's Vision reads in a picture.
    pub(super) fn read_codes(pixels: &CVPixelBuffer) -> Vec<String> {
        // SAFETY: Vision reads the picture it is handed and keeps nothing.
        unsafe {
            let Some(qr) = VNBarcodeSymbologyQR else {
                return Vec::new();
            };
            let request = VNDetectBarcodesRequest::new();
            request.setSymbologies(&NSArray::from_slice(&[qr]));
            let handler = VNImageRequestHandler::initWithCVPixelBuffer_options(
                VNImageRequestHandler::alloc(),
                pixels,
                &NSDictionary::<VNImageOption, AnyObject>::new(),
            );
            let requests = NSArray::<VNRequest>::from_slice(&[&request]);
            if handler.performRequests_error(&requests).is_err() {
                return Vec::new();
            }
            request
                .results()
                .map(|found| {
                    found
                        .iter()
                        .filter_map(|code| code.payloadStringValue().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default()
        }
    }

    /// A small mirrored copy of the picture, for aiming.
    unsafe fn preview(pixels: &CVPixelBuffer) -> Option<Handle> {
        if CVPixelBufferGetPixelFormatType(pixels) != kCVPixelFormatType_32BGRA {
            return None;
        }
        if CVPixelBufferLockBaseAddress(pixels, CVPixelBufferLockFlags::ReadOnly) != kCVReturnSuccess
        {
            return None;
        }
        let width = CVPixelBufferGetWidth(pixels);
        let height = CVPixelBufferGetHeight(pixels);
        let stride = CVPixelBufferGetBytesPerRow(pixels);
        let base = CVPixelBufferGetBaseAddress(pixels) as *const u8;
        let result = (!base.is_null() && width > 0 && height > 0 && stride >= width * 4).then(|| {
            let bgra = std::slice::from_raw_parts(base, stride * height);
            convert(bgra, width, height, stride)
        });
        CVPixelBufferUnlockBaseAddress(pixels, CVPixelBufferLockFlags::ReadOnly);
        result
    }

    fn convert(bgra: &[u8], width: usize, height: usize, stride: usize) -> Handle {
        let step = width.div_ceil(PREVIEW_WIDTH).max(1);
        let (pw, ph) = (width / step, height / step);
        let mut rgba = Vec::with_capacity(pw * ph * 4);
        for py in 0..ph {
            let row = &bgra[py * step * stride..];
            for px in 0..pw {
                // Mirrored, as a camera facing the user is expected to look.
                let p = &row[(width - 1 - px * step) * 4..][..4];
                rgba.extend_from_slice(&[p[2], p[1], p[0], 255]);
            }
        }
        Handle::from_rgba(pw as u32, ph as u32, rgba)
    }

    #[cfg(test)]
    pub(crate) mod tests {
        use super::*;

        /// The capture pieces fit together at run time: the delegate class,
        /// its selector and the picture settings. Nothing here asks for the
        /// camera or turns it on.
        #[test]
        fn the_capture_pieces_fit_together() {
            unsafe {
                let video = AVMediaTypeVideo.expect("AVFoundation media type");
                let _ = AVCaptureDevice::authorizationStatusForMediaType(video);
                let (sender, _events) = mpsc::channel(2);
                let delegate = Delegate::new(sender);
                let output = AVCaptureVideoDataOutput::new();
                let key: &NSString =
                    &*(kCVPixelBufferPixelFormatTypeKey as *const _ as *const NSString);
                let bgra = NSNumber::new_u32(kCVPixelFormatType_32BGRA);
                let settings = NSDictionary::<NSString, AnyObject>::from_slices(&[key], &[&*bgra]);
                output.setVideoSettings(Some(&settings));
                let queue = DispatchQueue::new("talia.camera.test", None);
                output.setSampleBufferDelegate_queue(
                    Some(ProtocolObject::from_ref(&*delegate)),
                    Some(&queue),
                );
                assert!(output.sampleBufferDelegate().is_some());
                output.setSampleBufferDelegate_queue(None, None);
            }
        }

        #[test]
        fn the_preview_is_small_and_mirrored() {
            // 4x1 picture: red, green, blue, white (BGRA).
            let bgra = [0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 255, 255, 255];
            let Handle::Rgba { width, height, pixels, .. } = convert(&bgra, 4, 1, 16) else {
                panic!("an RGBA preview");
            };
            assert_eq!((width, height), (4, 1));
            // Mirrored: white first, red last.
            assert_eq!(&pixels[..4], &[255, 255, 255, 255]);
            assert_eq!(&pixels[12..], &[255, 0, 0, 255]);
        }

        /// A picture in a pixel buffer, as the camera hands it over.
        pub(crate) fn pixel_buffer(
            width: usize,
            height: usize,
            luma: &[u8],
        ) -> objc2_core_foundation::CFRetained<CVPixelBuffer> {
            unsafe {
                let mut out: *mut CVPixelBuffer = std::ptr::null_mut();
                assert_eq!(
                    objc2_core_video::CVPixelBufferCreate(
                        None,
                        width,
                        height,
                        kCVPixelFormatType_32BGRA,
                        None,
                        std::ptr::NonNull::from(&mut out),
                    ),
                    kCVReturnSuccess
                );
                let pixels = objc2_core_foundation::CFRetained::from_raw(
                    std::ptr::NonNull::new(out).unwrap(),
                );
                CVPixelBufferLockBaseAddress(&pixels, CVPixelBufferLockFlags(0));
                let stride = CVPixelBufferGetBytesPerRow(&pixels);
                let base = CVPixelBufferGetBaseAddress(&pixels) as *mut u8;
                for y in 0..height {
                    for x in 0..width {
                        let v = luma[y * width + x];
                        let p = base.add(y * stride + x * 4);
                        *p = v;
                        *p.add(1) = v;
                        *p.add(2) = v;
                        *p.add(3) = 255;
                    }
                }
                CVPixelBufferUnlockBaseAddress(&pixels, CVPixelBufferLockFlags(0));
                pixels
            }
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::mac::{read_codes, tests::pixel_buffer};

    /// A frame of the backup code on a 1920x1080 picture, as a phone held up
    /// to the camera shows it: sharp, then blurred as a webcam blurs a
    /// screen too close to it.
    #[test]
    fn a_dense_frame_is_read_sharp_and_blurred() {
        let frames = crate::bbqr::encode(&[b'x'; 500], crate::bbqr::FileType::Text).unwrap();
        let (width, height) = (1920usize, 1080usize);
        let (side, code) = qrcode_luma(&frames[0], 5);
        let mut picture = vec![170u8; width * height];
        let (ox, oy) = ((width - side) / 2, (height - side) / 2);
        for y in 0..side {
            picture[(oy + y) * width + ox..(oy + y) * width + ox + side]
                .copy_from_slice(&code[y * side..(y + 1) * side]);
        }
        let sharp = pixel_buffer(width, height, &picture);
        assert_eq!(read_codes(&sharp), vec![frames[0].clone()]);

        let blurred = pixel_buffer(width, height, &box_blur(&picture, width, height, 2));
        assert_eq!(read_codes(&blurred), vec![frames[0].clone()]);

        let empty = pixel_buffer(width, height, &vec![170u8; width * height]);
        assert!(read_codes(&empty).is_empty());
    }

    fn box_blur(luma: &[u8], width: usize, height: usize, r: usize) -> Vec<u8> {
        let mut out = luma.to_vec();
        for y in r..height - r {
            for x in r..width - r {
                let mut sum = 0u32;
                for dy in 0..=2 * r {
                    for dx in 0..=2 * r {
                        sum += u32::from(luma[(y + dy - r) * width + x + dx - r]);
                    }
                }
                out[y * width + x] = (sum / ((2 * r + 1) * (2 * r + 1)) as u32) as u8;
            }
        }
        out
    }

    /// A QR code drawn as greyscale pixels, `scale` pixels a module, with
    /// its quiet zone.
    fn qrcode_luma(text: &str, scale: usize) -> (usize, Vec<u8>) {
        let code = qrcode::QrCode::with_error_correction_level(text, qrcode::EcLevel::L).unwrap();
        let modules = code.width();
        let side = (modules + 8) * scale;
        let mut luma = vec![255u8; side * side];
        for (i, cell) in code.to_colors().iter().enumerate() {
            if matches!(cell, qrcode::Color::Dark) {
                let (mx, my) = (i % modules + 4, i / modules + 4);
                for y in 0..scale {
                    for x in 0..scale {
                        luma[(my * scale + y) * side + mx * scale + x] = 0;
                    }
                }
            }
        }
        (side, luma)
    }
}
