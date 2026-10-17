//! Minimal local HTTP/1.1 fixture server for auth tests. Test builds only; the
//! production client refuses plain HTTP.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: HashMap<String, String>,
    pub body: String,
}

pub type Handler = Arc<dyn Fn(&Recorded) -> (u16, String) + Send + Sync>;

pub struct TestServer {
    pub base: String,
    pub requests: Arc<Mutex<Vec<Recorded>>>,
    handler: Arc<Mutex<Handler>>,
    /// Delay applied before answering requests whose path matches.
    pub delays: Arc<Mutex<HashMap<String, Duration>>>,
}

impl TestServer {
    pub async fn start(handler: Handler) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let handler = Arc::new(Mutex::new(handler));
        let delays: Arc<Mutex<HashMap<String, Duration>>> = Arc::default();
        let (r, h, d) = (
            Arc::clone(&requests),
            Arc::clone(&handler),
            Arc::clone(&delays),
        );
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let (r, h, d) = (Arc::clone(&r), Arc::clone(&h), Arc::clone(&d));
                tokio::spawn(async move {
                    let Some(request) = read_request(&mut stream).await else {
                        return;
                    };
                    let delay = d.lock().unwrap().get(&request.path).copied();
                    if let Some(delay) = delay {
                        tokio::time::sleep(delay).await;
                    }
                    r.lock().unwrap().push(request.clone());
                    let handler = Arc::clone(&*h.lock().unwrap());
                    let (status, body) = handler(&request);
                    let response = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        Self {
            base,
            requests,
            handler,
            delays,
        }
    }

    pub fn set_handler(&self, handler: Handler) {
        *self.handler.lock().unwrap() = handler;
    }

    pub fn count(&self, path: &str) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path == path)
            .count()
    }

    pub fn last(&self, path: &str) -> Option<Recorded> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|r| r.path == path)
            .cloned()
    }
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> Option<Recorded> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut lines = head.lines();
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_owned();
    let target = first.next()?.to_owned();
    let headers: HashMap<String, String> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let length: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    while buffer.len() < head_end + length {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..n]);
    }
    let body = String::from_utf8_lossy(&buffer[head_end..]).into_owned();
    let (path, query) = target
        .split_once('?')
        .map(|(p, q)| (p.to_owned(), q.to_owned()))
        .unwrap_or((target, String::new()));
    Some(Recorded {
        method,
        path,
        query,
        headers,
        body,
    })
}
