//! HTTP/1.1 for the smithy and its arenas, both sides: a blocking listener that hands each request to
//! a handler (`Listener::serve`), and one request at a time from a client (`get`, `post`). Small
//! synchronous crates, `tiny_http` and `ureq`, under it, so neither binary writes HTTP by hand.
//!
//! The protocol's answers are all `200`, their body starting with a status word line
//! (`temper_chess::pipeline::protocol`); the HTTP code says only what HTTP knows: a malformed request
//! (`400`), an unknown path (`404`), a body over `BODY_LIMIT` (`413`).

use std::io::Read;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use temper_chess::pipeline::protocol::{LOOPBACK, STOP_PATH};

/// The header every request after `hello` carries its token in.
pub const TOKEN_HEADER: &str = "X-Temper-Token";

/// The largest body either side takes: a job's compressed games or a network are a few MB.
pub const BODY_LIMIT: u64 = 64 * 1024 * 1024;

/// How long `serve` waits for a request before checking its stop flag again.
const POLL: Duration = Duration::from_millis(100);

/// One request, as a handler sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub token: Option<String>,
    /// The `Host` header, which a loopback server checks against DNS rebinding.
    pub host: Option<String>,
    pub body: Vec<u8>,
}

/// A handler's answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub code: u16,
    pub body: Vec<u8>,
}

impl Reply {
    /// A protocol answer: `200`, the status word on its own line, then `body`.
    pub fn status(word: &str, body: &[u8]) -> Reply {
        let mut bytes = format!("{word}\n").into_bytes();
        bytes.extend_from_slice(body);
        Reply { code: 200, body: bytes }
    }

    /// An HTTP-level refusal, `code` with a line saying why.
    pub fn error(code: u16, message: &str) -> Reply {
        Reply { code, body: format!("{message}\n").into_bytes() }
    }
}

/// A listening socket, never on `0.0.0.0`: the caller names the address.
pub struct Listener {
    server: tiny_http::Server,
}

impl Listener {
    /// Listens on `host:port`, `host` a name or an address, its first IPv4 address taken.
    pub fn bind(host: &str, port: u16) -> Result<Listener, String> {
        let address = ipv4(host, port)?;
        let server = tiny_http::Server::http(address).map_err(|error| format!("cannot listen on {address}: {error}"))?;
        Ok(Listener { server })
    }

    /// The port listened on: the one asked for, or the system's choice for `0`.
    pub fn port(&self) -> u16 {
        self.server.server_addr().to_ip().map_or(0, |address| address.port())
    }

    /// Answers every request with `handle` until `stop` is raised.
    pub fn serve(&self, handle: &dyn Fn(&Request) -> Reply, stop: &AtomicBool) {
        while !stop.load(Ordering::SeqCst) {
            let mut request = match self.server.recv_timeout(POLL) {
                Ok(Some(request)) => request,
                Ok(None) => continue,
                Err(_) => {
                    std::thread::sleep(POLL);
                    continue;
                }
            };
            let reply = match read(&mut request) {
                Ok(parsed) => handle(&parsed),
                Err(reply) => reply,
            };
            // A client gone before its answer costs it only a retry.
            let _ = request.respond(tiny_http::Response::from_data(reply.body).with_status_code(reply.code));
        }
    }

    /// As `serve`, `threads` requests at a time: a slow one (a page reading the remote) then holds up
    /// its own thread alone, not every caller queued behind it.
    pub fn serve_concurrently(&self, threads: usize, handle: &(dyn Fn(&Request) -> Reply + Sync), stop: &AtomicBool) {
        std::thread::scope(|scope| {
            for _ in 0..threads.max(1) {
                scope.spawn(|| self.serve(handle, stop));
            }
        });
    }
}

/// `request` as a handler sees it, or the refusal to send instead.
fn read(request: &mut tiny_http::Request) -> Result<Request, Reply> {
    if request.body_length().is_some_and(|length| length as u64 > BODY_LIMIT) {
        return Err(Reply::error(413, "the body is too large"));
    }
    let mut body = Vec::new();
    request
        .as_reader()
        .take(BODY_LIMIT + 1)
        .read_to_end(&mut body)
        .map_err(|error| Reply::error(400, &format!("reading the body: {error}")))?;
    if body.len() as u64 > BODY_LIMIT {
        return Err(Reply::error(413, "the body is too large"));
    }
    let header = |name: &'static str| -> Option<String> {
        request.headers().iter().find(|header| header.field.equiv(name)).map(|header| header.value.as_str().to_string())
    };
    let (token, host) = (header(TOKEN_HEADER), header("Host"));
    Ok(Request { method: request.method().as_str().to_string(), path: request.url().to_string(), token, host, body })
}

fn ipv4(host: &str, port: u16) -> Result<SocketAddr, String> {
    (host, port)
        .to_socket_addrs()
        .map_err(|error| format!("{host}: {error}"))?
        .find(SocketAddr::is_ipv4)
        .ok_or_else(|| format!("{host}: no IPv4 address"))
}

/// `http://host:port/path`.
pub fn url(host: &str, port: u16, path: &str) -> String {
    format!("http://{host}:{port}{path}")
}

/// A `GET` of `url`: its HTTP code and body, or why it could not be made.
pub fn get(url: &str, token: Option<&str>, timeout: Duration) -> Result<Reply, String> {
    let mut request = agent(timeout).get(url);
    if let Some(token) = token {
        request = request.set(TOKEN_HEADER, token);
    }
    answer(url, request.call())
}

/// A `POST` of `body` to `url`: its HTTP code and body, or why it could not be made.
pub fn post(url: &str, token: Option<&str>, body: &[u8], timeout: Duration) -> Result<Reply, String> {
    let mut request = agent(timeout).post(url);
    if let Some(token) = token {
        request = request.set(TOKEN_HEADER, token);
    }
    answer(url, request.send_bytes(body))
}

/// An agent connecting to IPv4 addresses alone, as `Listener::bind` listens. Windows resolves its own
/// machine's name to its IPv6 addresses first: a connection there is refused, and Windows retries a
/// refused connection for about 2 s before ureq tries IPv4, which cost every request of an arena on
/// the smithy's own machine those 2 s.
fn agent(timeout: Duration) -> ureq::Agent {
    let ipv4_only = |address: &str| -> std::io::Result<Vec<SocketAddr>> {
        Ok(address.to_socket_addrs()?.filter(SocketAddr::is_ipv4).collect())
    };
    ureq::AgentBuilder::new().resolver(ipv4_only).timeout_connect(timeout).timeout_read(timeout).timeout_write(timeout).build()
}

/// Any HTTP code is an answer; only a request that got none is an error.
fn answer(url: &str, result: Result<ureq::Response, ureq::Error>) -> Result<Reply, String> {
    let response = match result {
        Ok(response) => response,
        Err(ureq::Error::Status(_, response)) => response,
        Err(ureq::Error::Transport(transport)) => return Err(format!("{url}: {transport}")),
    };
    let code = response.status();
    let mut body = Vec::new();
    response.into_reader().take(BODY_LIMIT + 1).read_to_end(&mut body).map_err(|error| format!("{url}: {error}"))?;
    if body.len() as u64 > BODY_LIMIT {
        return Err(format!("{url}: the answer is over {BODY_LIMIT} bytes"));
    }
    Ok(Reply { code, body })
}

/// A protocol answer's status word and the body after it; `None` when `reply` is not a `200`.
pub fn status_of(reply: &Reply) -> Option<(String, Vec<u8>)> {
    if reply.code != 200 {
        return None;
    }
    let end = reply.body.iter().position(|&byte| byte == b'\n').unwrap_or(reply.body.len());
    let word = String::from_utf8_lossy(&reply.body[..end]).to_string();
    Some((word, reply.body.get(end + 1..).unwrap_or(&[]).to_vec()))
}

/// A control request to this machine's process listening on `port`, `STATUS_PATH` or `STOP_PATH`:
/// its answer's body, or `none_running` when nothing answers.
pub fn control_request(port: u16, path: &str, none_running: &str) -> Result<String, String> {
    let url = url(LOOPBACK, port, path);
    let timeout = Duration::from_secs(10);
    let reply = if path == STOP_PATH { post(&url, None, b"", timeout) } else { get(&url, None, timeout) };
    let Ok(reply) = reply else {
        return Err(none_running.to_string());
    };
    let Some((word, body)) = status_of(&reply) else {
        return Err(format!("it answered {}: {}", reply.code, String::from_utf8_lossy(&reply.body).trim()));
    };
    if word != "OK" {
        return Err(format!("it answered {word}"));
    }
    Ok(String::from_utf8_lossy(&body).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A server on `127.0.0.1:0` answering with `handle` on a thread of its own, and its port; the
    /// flag stops it.
    fn spawn(handle: fn(&Request) -> Reply) -> (u16, Arc<AtomicBool>, std::thread::JoinHandle<()>) {
        let listener = Listener::bind("127.0.0.1", 0).unwrap();
        let port = listener.port();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || listener.serve(&handle, &stop))
        };
        (port, stop, thread)
    }

    fn echo(request: &Request) -> Reply {
        if request.path == "/missing" {
            return Reply::error(404, "no such path");
        }
        let text = format!("{} {} {:?}\n", request.method, request.path, request.token);
        let mut body = text.into_bytes();
        body.extend_from_slice(&request.body);
        Reply::status("OK", &body)
    }

    #[test]
    fn a_request_reaches_its_handler_and_its_answer_comes_back() {
        let (port, stop, thread) = spawn(echo);
        let timeout = Duration::from_secs(5);
        let reply = post(&url("127.0.0.1", port, "/work"), Some("t0k3n"), &[0, 159, b'\n', 255], timeout).unwrap();
        let (word, body) = status_of(&reply).unwrap();
        assert_eq!(word, "OK");
        assert_eq!(body, [&b"POST /work Some(\"t0k3n\")\n"[..], &[0, 159, b'\n', 255]].concat(), "bytes survive both ways");
        let reply = get(&url("127.0.0.1", port, "/hello"), None, timeout).unwrap();
        assert_eq!(status_of(&reply).unwrap().1, b"GET /hello None\n");
        let reply = get(&url("127.0.0.1", port, "/missing"), None, timeout).unwrap();
        assert_eq!((reply.code, status_of(&reply)), (404, None));
        stop.store(true, Ordering::SeqCst);
        thread.join().unwrap();
    }

    #[test]
    fn a_slow_request_does_not_hold_up_a_quick_one_when_served_concurrently() {
        let listener = Listener::bind("127.0.0.1", 0).unwrap();
        let port = listener.port();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = Arc::clone(&stop);
            let handle = |request: &Request| {
                if request.path == "/slow" {
                    std::thread::sleep(Duration::from_secs(3));
                }
                Reply::status("OK", b"")
            };
            std::thread::spawn(move || listener.serve_concurrently(2, &handle, &stop))
        };
        let timeout = Duration::from_secs(10);
        let slow = std::thread::spawn(move || get(&url("127.0.0.1", port, "/slow"), None, timeout));
        std::thread::sleep(Duration::from_millis(300));
        let started = std::time::Instant::now();
        assert!(get(&url("127.0.0.1", port, "/quick"), None, timeout).is_ok());
        assert!(started.elapsed() < Duration::from_secs(1), "the quick request waited {:?}", started.elapsed());
        assert!(slow.join().unwrap().is_ok());
        stop.store(true, Ordering::SeqCst);
        thread.join().unwrap();
    }

    #[test]
    fn a_request_to_nobody_is_an_error_not_an_answer() {
        let listener = Listener::bind("127.0.0.1", 0).unwrap();
        let port = listener.port();
        drop(listener);
        assert!(get(&url("127.0.0.1", port, "/"), None, Duration::from_secs(2)).is_err());
    }
}
