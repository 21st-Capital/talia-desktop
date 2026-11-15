//! Talia Desktop: reading QR codes with the computer's camera, for a vault
//! that comes from Talia on the iPhone (decided 2026-11-15).
//!
//! On the Mac, Apple's AVFoundation films at 1280x720 and hands each picture
//! to a pure-Rust QR reader (`rqrr`). Only text read from codes and a small
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

/// The QR codes in a greyscale picture.
#[cfg(target_os = "macos")]
pub fn read_codes(width: usize, height: usize, luma: &[u8]) -> Vec<String> {
    if luma.len() < width * height {
        return Vec::new();
    }
    let mut prepared =
        rqrr::PreparedImage::prepare_from_greyscale(width, height, |x, y| luma[y * width + x]);
    prepared
        .detect_grids()
        .iter()
        .filter_map(|grid| grid.decode().ok().map(|(_, text)| text))
        .collect()
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
        rc::Retained,
        runtime::{AnyObject, Bool, ProtocolObject},
        AllocAnyThread, DefinedClass,
    };
    use objc2_av_foundation::{
        AVAuthorizationStatus, AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceInput,
        AVCaptureOutput, AVCaptureSession, AVCaptureSessionPreset1280x720,
        AVCaptureVideoDataOutput, AVCaptureVideoDataOutputSampleBufferDelegate, AVMediaTypeVideo,
    };
    use objc2_core_media::CMSampleBuffer;
    use objc2_core_video::{
        kCVPixelBufferPixelFormatTypeKey, kCVPixelFormatType_32BGRA, kCVReturnSuccess,
        CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight,
        CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
        CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    };
    use objc2_foundation::{NSDictionary, NSNumber, NSObject, NSObjectProtocol, NSString};
    use tokio::sync::{mpsc, oneshot};

    use super::{read_codes, Access, Event};

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
            if session.canSetSessionPreset(AVCaptureSessionPreset1280x720) {
                session.setSessionPreset(AVCaptureSessionPreset1280x720);
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
            // SAFETY: the buffer is locked while it is read and never kept.
            let Some((luma, width, height, preview)) = (unsafe { read_picture(sample_buffer) })
            else {
                return;
            };
            let codes = read_codes(width, height, &luma);
            let _ = ivars.sender.try_send(Event::Frame { preview, codes });
        }
    }

    /// The picture in greyscale at full size, and a mirrored preview.
    unsafe fn read_picture(sample_buffer: &CMSampleBuffer) -> Option<(Vec<u8>, usize, usize, Handle)> {
        let pixels = sample_buffer.image_buffer()?;
        if CVPixelBufferGetPixelFormatType(&pixels) != kCVPixelFormatType_32BGRA {
            return None;
        }
        if CVPixelBufferLockBaseAddress(&pixels, CVPixelBufferLockFlags::ReadOnly) != kCVReturnSuccess
        {
            return None;
        }
        let width = CVPixelBufferGetWidth(&pixels);
        let height = CVPixelBufferGetHeight(&pixels);
        let stride = CVPixelBufferGetBytesPerRow(&pixels);
        let base = CVPixelBufferGetBaseAddress(&pixels) as *const u8;
        let result = (!base.is_null() && width > 0 && height > 0 && stride >= width * 4).then(|| {
            let bgra = std::slice::from_raw_parts(base, stride * height);
            convert(bgra, width, height, stride)
        });
        CVPixelBufferUnlockBaseAddress(&pixels, CVPixelBufferLockFlags::ReadOnly);
        result.map(|(luma, preview)| (luma, width, height, preview))
    }

    fn convert(bgra: &[u8], width: usize, height: usize, stride: usize) -> (Vec<u8>, Handle) {
        let mut luma = Vec::with_capacity(width * height);
        for y in 0..height {
            let row = &bgra[y * stride..y * stride + width * 4];
            luma.extend(row.chunks_exact(4).map(|p| {
                ((u32::from(p[2]) * 77 + u32::from(p[1]) * 150 + u32::from(p[0]) * 29) >> 8) as u8
            }));
        }
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
        (luma, Handle::from_rgba(pw as u32, ph as u32, rgba))
    }

    #[cfg(test)]
    mod tests {
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
            let (luma, _) = convert(&bgra, 4, 1, 16);
            assert_eq!(luma.len(), 4);
            assert!(luma[3] > 250 && luma[0] < 90);
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    /// A frame of the backup code as it looks on a screen, read at the
    /// camera's size.
    #[test]
    fn a_dense_frame_is_read_at_camera_size() {
        let frames = crate::bbqr::encode(&[b'x'; 500], crate::bbqr::FileType::Text).unwrap();
        let code = qrcode_luma(&frames[0], 6);
        let (side, luma) = code;
        // On a 1280x720 picture, as a phone held up to the camera shows it.
        let (width, height) = (1280usize, 720usize);
        let mut picture = vec![200u8; width * height];
        let (ox, oy) = ((width - side) / 2, (height - side) / 2);
        for y in 0..side {
            picture[(oy + y) * width + ox..(oy + y) * width + ox + side]
                .copy_from_slice(&luma[y * side..(y + 1) * side]);
        }
        assert_eq!(read_codes(width, height, &picture), vec![frames[0].clone()]);
        assert!(read_codes(width, height, &vec![200u8; width * height]).is_empty());
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
