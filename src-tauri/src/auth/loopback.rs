//! Short-lived loopback listener for the OAuth redirect (docs/spec/03 "System-browser
//! OAuth implementation", "OAuth callback and account concurrency details").
//!
//! The listener binds only loopback addresses, before the browser opens. It accepts
//! exactly one valid callback: the registered path with a matching `state`. Other
//! paths, wrong or duplicated parameters and malformed requests are answered and
//! ignored without consuming the attempt. Requests are size- and time-bounded, and
//! the callback query never reaches logs or the UI.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use super::contract;
use super::pkce::secrets_equal;
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::secrets::SecretString;

const MAX_REQUEST_BYTES: usize = 8 * 1024;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

/// The single accepted result of an attempt.
#[derive(Debug)]
pub enum Callback {
    Code(SecretString),
    /// The provider redirected with an OAuth error (for example consent denied).
    ProviderError(String),
}

pub struct LoopbackServer {
    v4: TcpListener,
    v6: Option<TcpListener>,
    port: u16,
}

impl LoopbackServer {
    /// Binds 127.0.0.1 on an ephemeral port and, when available, ::1 on the same
    /// port, because `localhost` may resolve to either. Never binds 0.0.0.0.
    pub async fn bind() -> LauncherResult<Self> {
        let v4 = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .map_err(|e| LauncherError::from_io(&e, "Preparing the sign-in callback"))?;
        let port = v4
            .local_addr()
            .map_err(|e| LauncherError::from_io(&e, "Preparing the sign-in callback"))?
            .port();
        let v6 = TcpListener::bind(SocketAddr::from((Ipv6Addr::LOCALHOST, port)))
            .await
            .ok();
        Ok(Self { v4, v6, port })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn redirect_uri(&self) -> String {
        format!(
            "http://{}:{}{}",
            contract::REDIRECT_HOST,
            self.port,
            contract::REDIRECT_PATH
        )
    }

    /// Waits for the matching callback, a timeout or cancellation. The listener is
    /// dropped when this returns, so later callbacks find nothing listening.
    pub async fn wait(
        self,
        expected_state: &SecretString,
        timeout: Duration,
        mut cancel: watch::Receiver<bool>,
    ) -> LauncherResult<Callback> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let accepted = tokio::select! {
                result = self.v4.accept() => result.ok().map(|(s, _)| s),
                result = accept_optional(self.v6.as_ref()) => result,
                () = tokio::time::sleep_until(deadline) => {
                    return Err(LauncherError::new(
                        ErrorCode::AuthTimeout,
                        "Sign-in was not completed in time. Start sign-in again.",
                    ));
                }
                changed = cancel.changed() => {
                    if changed.is_err() || *cancel.borrow() {
                        return Err(LauncherError::new(
                            ErrorCode::AuthCancelled,
                            "Sign-in was cancelled.",
                        ));
                    }
                    continue;
                }
            };
            let Some(stream) = accepted else { continue };
            if let Some(callback) = handle(stream, expected_state).await {
                return Ok(callback);
            }
        }
    }
}

async fn accept_optional(listener: Option<&TcpListener>) -> Option<TcpStream> {
    match listener {
        Some(listener) => listener.accept().await.ok().map(|(s, _)| s),
        None => std::future::pending().await,
    }
}

/// Reads one request and answers it. Returns a callback only for a valid match.
async fn handle(mut stream: TcpStream, expected_state: &SecretString) -> Option<Callback> {
    let request = tokio::time::timeout(CONNECTION_TIMEOUT, read_head(&mut stream))
        .await
        .ok()
        .flatten();
    let (status, body, result) = match request.as_deref().map(|r| evaluate(r, expected_state)) {
        Some(Evaluation::Accepted(callback)) => {
            let body = match &callback {
                Callback::Code(_) => {
                    "Sign-in received. You can close this window and return to \
                                      the launcher."
                }
                Callback::ProviderError(_) => {
                    "Sign-in was not completed. You can close this \
                                               window and return to the launcher."
                }
            };
            ("200 OK", body, Some(callback))
        }
        Some(Evaluation::NotFound) => ("404 Not Found", "Not found.", None),
        Some(Evaluation::Rejected) | None => ("400 Bad Request", "Invalid request.", None),
    };
    let page = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Kurasis \
         Launcher</title></head><body><p>{body}</p></body></html>"
    );
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: \
         default-src 'none'\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );
    let _ = tokio::time::timeout(CONNECTION_TIMEOUT, stream.write_all(response.as_bytes())).await;
    let _ = stream.shutdown().await;
    result
}

async fn read_head(stream: &mut TcpStream) -> Option<String> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.windows(4).any(|w| w == b"\r\n\r\n") {
            return String::from_utf8(buffer).ok();
        }
        if buffer.len() > MAX_REQUEST_BYTES {
            return None;
        }
    }
}

enum Evaluation {
    Accepted(Callback),
    NotFound,
    Rejected,
}

fn evaluate(request: &str, expected_state: &SecretString) -> Evaluation {
    let Some(line) = request.lines().next() else {
        return Evaluation::Rejected;
    };
    let mut parts = line.split(' ');
    let (Some("GET"), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Evaluation::Rejected;
    };
    if !version.starts_with("HTTP/1.") {
        return Evaluation::Rejected;
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != contract::REDIRECT_PATH {
        return Evaluation::NotFound;
    }
    let mut state = None;
    let mut code = None;
    let mut error = None;
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        let slot = match key.as_ref() {
            "state" => &mut state,
            "code" => &mut code,
            "error" => &mut error,
            _ => continue,
        };
        if slot.is_some() {
            return Evaluation::Rejected; // duplicated parameter
        }
        *slot = Some(value.into_owned());
    }
    let Some(state) = state else {
        return Evaluation::Rejected;
    };
    if !secrets_equal(&state, expected_state.expose()) {
        return Evaluation::Rejected;
    }
    match (code, error) {
        (Some(code), None) if !code.is_empty() => {
            Evaluation::Accepted(Callback::Code(SecretString::new(code)))
        }
        (None, Some(error)) => Evaluation::Accepted(Callback::ProviderError(
            // Only a short, safe identifier is kept, never the description.
            error
                .chars()
                .take(64)
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect(),
        )),
        _ => Evaluation::Rejected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn send(port: u16, request: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response).await;
        response
    }

    fn get(path: &str) -> String {
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n")
    }

    #[tokio::test]
    async fn accepts_only_the_matching_callback() {
        let server = LoopbackServer::bind().await.unwrap();
        let port = server.port();
        assert!(server.redirect_uri().starts_with("http://localhost:"));
        let state = SecretString::new("expected-state");
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let waiter = tokio::spawn(async move {
            server
                .wait(&state, Duration::from_secs(10), cancel_rx)
                .await
        });

        assert!(
            send(port, &get("/favicon.ico"))
                .await
                .starts_with("HTTP/1.1 404")
        );
        assert!(
            send(port, &get("/?code=x&state=wrong"))
                .await
                .starts_with("HTTP/1.1 400")
        );
        assert!(
            send(port, &get("/?code=x&code=y&state=expected-state"))
                .await
                .starts_with("HTTP/1.1 400")
        );
        assert!(
            send(port, "garbage\r\n\r\n")
                .await
                .starts_with("HTTP/1.1 400")
        );
        let ok = send(port, &get("/?code=the-code&state=expected-state")).await;
        assert!(ok.starts_with("HTTP/1.1 200"));
        assert!(ok.contains("Cache-Control: no-store"));
        assert!(!ok.contains("the-code"), "the code is never echoed");

        match waiter.await.unwrap().unwrap() {
            Callback::Code(code) => assert_eq!(code.expose(), "the-code"),
            other => panic!("{other:?}"),
        }
        // The attempt is over: nothing listens any more.
        assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    }

    #[tokio::test]
    async fn provider_error_ends_the_attempt() {
        let server = LoopbackServer::bind().await.unwrap();
        let port = server.port();
        let state = SecretString::new("s");
        let (_tx, rx) = watch::channel(false);
        let waiter =
            tokio::spawn(async move { server.wait(&state, Duration::from_secs(10), rx).await });
        send(
            port,
            &get("/?error=access_denied&error_description=user+said+no&state=s"),
        )
        .await;
        match waiter.await.unwrap().unwrap() {
            Callback::ProviderError(error) => assert_eq!(error, "access_denied"),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn timeout_and_cancel_are_reported() {
        let server = LoopbackServer::bind().await.unwrap();
        let (_tx, rx) = watch::channel(false);
        let error = server
            .wait(&SecretString::new("s"), Duration::from_millis(100), rx)
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::AuthTimeout);

        let server = LoopbackServer::bind().await.unwrap();
        let port = server.port();
        let (tx, rx) = watch::channel(false);
        let waiter = tokio::spawn(async move {
            server
                .wait(&SecretString::new("s"), Duration::from_secs(10), rx)
                .await
        });
        tx.send(true).unwrap();
        assert_eq!(
            waiter.await.unwrap().unwrap_err().code,
            ErrorCode::AuthCancelled
        );
        // A late callback after cancel reaches no listener.
        assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    }

    #[tokio::test]
    async fn oversized_requests_are_rejected() {
        let server = LoopbackServer::bind().await.unwrap();
        let port = server.port();
        let (_tx, rx) = watch::channel(false);
        let waiter = tokio::spawn(async move {
            server
                .wait(&SecretString::new("s"), Duration::from_secs(5), rx)
                .await
        });
        let huge = format!("GET /?state=s&code={} HTTP/1.1\r\n\r\n", "a".repeat(10_000));
        let response = send(port, &huge).await;
        assert!(response.is_empty() || response.starts_with("HTTP/1.1 400"));
        send(port, &get("/?state=s&code=ok")).await;
        assert!(matches!(waiter.await.unwrap().unwrap(), Callback::Code(_)));
    }
}
