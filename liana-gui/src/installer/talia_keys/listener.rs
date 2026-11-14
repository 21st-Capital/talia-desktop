//! The loopback end of the key handoff (RFC 8252): one listener on
//! 127.0.0.1, waiting for the website to send the browser back with a
//! one-time code.

use std::time::Duration;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::{timeout_at, Instant},
};

/// The only path the listener answers.
pub const CALLBACK_PATH: &str = "/talia/keys";

/// How long one browser connection may take to send its request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Nothing a browser sends for this page comes near this.
const MAX_REQUEST: usize = 8 * 1024;

/// The words of the pages the browser is answered with, in the app's language.
#[derive(Debug, Clone)]
pub struct BrowserPages {
    pub title: String,
    pub done: String,
    pub wrong_request: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenError {
    /// No good answer before the deadline.
    TimedOut,
    Io(String),
}

/// Binds the loopback listener on a port the system picks. Only 127.0.0.1,
/// never every interface.
pub fn bind_loopback() -> std::io::Result<std::net::TcpListener> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

/// Equal in time whatever the content, for the `state` check.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
}

/// What one request asked for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    /// `GET /talia/keys` with its code and state.
    Callback {
        code: Option<String>,
        state: Option<String>,
    },
    Other,
}

/// The request line of an HTTP request: `GET /talia/keys?code=..&state=.. HTTP/1.1`.
fn parse_request(head: &str) -> Request {
    let line = head.lines().next().unwrap_or_default();
    let mut parts = line.split(' ');
    let (Some("GET"), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next())
    else {
        return Request::Other;
    };
    if !version.starts_with("HTTP/1.") || parts.next().is_some() {
        return Request::Other;
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != CALLBACK_PATH {
        return Request::Other;
    }
    let Ok(url) = reqwest::Url::parse(&format!("http://127.0.0.1/?{query}")) else {
        return Request::Other;
    };
    let (mut code, mut state) = (None, None);
    for (name, value) in url.query_pairs() {
        match name.as_ref() {
            // A repeated parameter is not a request this app made.
            "code" if code.is_none() => code = Some(value.into_owned()),
            "state" if state.is_none() => state = Some(value.into_owned()),
            "code" | "state" => return Request::Other,
            _ => {}
        }
    }
    Request::Callback { code, state }
}

/// A one-time code as the backend makes them: base64url, no padding.
fn is_code(code: &str) -> bool {
    (16..=128).contains(&code.len())
        && code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{title}</title></head>\
         <body style=\"font-family:-apple-system,system-ui,sans-serif;background:#0f1513;color:#fff;\
         display:flex;min-height:90vh;align-items:center;justify-content:center;margin:0 16px\">\
         <p style=\"font-size:18px;max-width:30em;text-align:center\">{body}</p></body></html>",
        title = escape_html(title),
        body = escape_html(body),
    )
}

async fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\n\
         X-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

/// Reads one request head, up to its blank line.
async fn read_head(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        if buf.len() > MAX_REQUEST {
            return None;
        }
        let n = timeout_at(deadline, stream.read(&mut chunk))
            .await
            .ok()?
            .ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    String::from_utf8(buf).ok()
}

/// Serves the listener until one `GET /talia/keys` carries a code and the
/// expected `state`, then answers the browser and returns the code; the
/// listener is dropped with it, so nothing listens afterwards. Any other path
/// is a 404, and a callback with a wrong state is refused, while the wait goes
/// on until `deadline`.
pub async fn wait_for_code(
    listener: TcpListener,
    expected_state: &str,
    deadline: Instant,
    pages: &BrowserPages,
) -> Result<String, ListenError> {
    loop {
        let (mut stream, _) = match timeout_at(deadline, listener.accept()).await {
            Err(_) => return Err(ListenError::TimedOut),
            Ok(Err(e)) => return Err(ListenError::Io(e.to_string())),
            Ok(Ok(accepted)) => accepted,
        };
        let Some(head) = read_head(&mut stream).await else {
            continue;
        };
        match parse_request(&head) {
            Request::Callback {
                code: Some(code),
                state: Some(state),
            } if constant_time_eq(state.as_bytes(), expected_state.as_bytes())
                && is_code(&code) =>
            {
                respond(&mut stream, "200 OK", &page(&pages.title, &pages.done)).await;
                return Ok(code);
            }
            Request::Callback { .. } => {
                respond(
                    &mut stream,
                    "400 Bad Request",
                    &page(&pages.title, &pages.wrong_request),
                )
                .await;
            }
            Request::Other => {
                respond(&mut stream, "404 Not Found", "").await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pages() -> BrowserPages {
        BrowserPages {
            title: "Talia Desktop".to_string(),
            done: "Your keys are on their way to Talia Desktop. You can close this tab."
                .to_string(),
            wrong_request: "Start again from Talia Desktop.".to_string(),
        }
    }

    const CODE: &str = "Q2xhaW1Db2RlRm9yVGVzdHNPbmx5MTIzNDU2Nzg5MDEy";

    #[test]
    fn requests_are_read_strictly() {
        assert_eq!(
            parse_request(&format!(
                "GET /talia/keys?code={CODE}&state=abc HTTP/1.1\r\nHost: x\r\n\r\n"
            )),
            Request::Callback {
                code: Some(CODE.to_string()),
                state: Some("abc".to_string())
            }
        );
        assert_eq!(
            parse_request("GET /favicon.ico HTTP/1.1\r\n\r\n"),
            Request::Other
        );
        assert_eq!(
            parse_request("POST /talia/keys?code=a&state=b HTTP/1.1\r\n\r\n"),
            Request::Other
        );
        assert_eq!(
            parse_request("GET /talia/keys/?code=a HTTP/1.1\r\n\r\n"),
            Request::Other
        );
        assert_eq!(
            parse_request("GET /talia/keys?code=a&code=b&state=s HTTP/1.1\r\n\r\n"),
            Request::Other
        );
        assert_eq!(
            parse_request("GET /talia/keys?state=s HTTP/1.1\r\n\r\n"),
            Request::Callback {
                code: None,
                state: Some("s".to_string())
            }
        );
    }

    #[test]
    fn state_is_compared_in_constant_time() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(!constant_time_eq(b"", b"a"));
    }

    #[test]
    fn the_listener_binds_the_loopback_only() {
        let listener = bind_loopback().unwrap();
        let addr = listener.local_addr().unwrap();
        assert!(addr.ip().is_loopback());
        assert_ne!(addr.port(), 0);
    }

    async fn get(port: u16, target: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        stream
            .write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).await.unwrap();
        answer
    }

    #[tokio::test]
    async fn one_good_callback_ends_the_wait_and_the_listening() {
        let std_listener = bind_loopback().unwrap();
        let port = std_listener.local_addr().unwrap().port();
        let listener = TcpListener::from_std(std_listener).unwrap();
        let waiting = tokio::spawn(async move {
            wait_for_code(
                listener,
                "the-state",
                Instant::now() + Duration::from_secs(10),
                &pages(),
            )
            .await
        });

        assert!(get(port, "/favicon.ico").await.starts_with("HTTP/1.1 404"));
        let wrong = get(port, &format!("/talia/keys?code={CODE}&state=other")).await;
        assert!(wrong.starts_with("HTTP/1.1 400"), "{}", wrong);
        let bad_code = get(port, "/talia/keys?code=x&state=the-state").await;
        assert!(bad_code.starts_with("HTTP/1.1 400"), "{}", bad_code);
        let done = get(port, &format!("/talia/keys?code={CODE}&state=the-state")).await;
        assert!(done.starts_with("HTTP/1.1 200"), "{}", done);
        assert!(done.contains("You can close this tab."));

        assert_eq!(waiting.await.unwrap(), Ok(CODE.to_string()));
        // Nothing listens on the port any more.
        assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    }

    #[tokio::test]
    async fn the_wait_times_out() {
        let listener = TcpListener::from_std(bind_loopback().unwrap()).unwrap();
        let res = wait_for_code(
            listener,
            "s",
            Instant::now() + Duration::from_millis(50),
            &pages(),
        )
        .await;
        assert_eq!(res, Err(ListenError::TimedOut));
    }

    #[tokio::test]
    async fn a_silent_connection_does_not_hold_the_wait() {
        let std_listener = bind_loopback().unwrap();
        let port = std_listener.local_addr().unwrap().port();
        let listener = TcpListener::from_std(std_listener).unwrap();
        let waiting = tokio::spawn(async move {
            wait_for_code(
                listener,
                "s",
                Instant::now() + Duration::from_secs(30),
                &pages(),
            )
            .await
        });
        // A connection that never sends anything, then closes.
        let silent = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        drop(silent);
        let done = get(port, &format!("/talia/keys?code={CODE}&state=s")).await;
        assert!(done.starts_with("HTTP/1.1 200"));
        assert_eq!(waiting.await.unwrap(), Ok(CODE.to_string()));
    }
}
