//! Local HTTP responses for the adapter and process tests.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use http::StatusCode;

pub(crate) struct Server {
    pub(crate) url: String,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Server {
    pub(crate) fn new(status: u16) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a free loopback port");
        listener.set_nonblocking(true).expect("nonblocking accept");
        let url = format!(
            "http://{}",
            listener.local_addr().expect("the bound address")
        );
        let location = format!("{url}/elsewhere");
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let worker = thread::spawn(move || serve(&listener, &stopping, status, &location));
        Self {
            url,
            stop,
            worker: Some(worker),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            worker.join().expect("the fixture server completed");
        }
    }
}

fn serve(listener: &TcpListener, stopping: &AtomicBool, status: u16, location: &str) {
    let mut first = true;
    while !stopping.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                // A followed redirect gets a distinct final answer, as in the probe.
                answer(stream, if first { status } else { 200 }, location);
                first = false;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!("accept: {error}"),
        }
    }
}

fn answer(stream: TcpStream, status: u16, location: &str) {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("a read timeout");
    let mut reader = BufReader::new(stream);
    if read_request(&mut reader).starts_with("CONNECT ") {
        reader
            .get_mut()
            .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
            .expect("the local tunnel opens");
        read_request(&mut reader);
    }
    let status = StatusCode::from_u16(status).expect("an HTTP status");
    let body = if status == StatusCode::NOT_MODIFIED {
        ""
    } else {
        "{}"
    };
    write!(reader.get_mut(),
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nLocation: {location}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()
    ).expect("the response is sent");
}

fn read_request(reader: &mut BufReader<TcpStream>) -> String {
    let mut request = String::new();
    reader.read_line(&mut request).expect("the request line");
    let mut length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).expect("request headers");
        if line == "\r\n" || line.is_empty() {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse().expect("a content length");
        }
    }
    reader
        .read_exact(&mut vec![0; length])
        .expect("request body");
    request
}
