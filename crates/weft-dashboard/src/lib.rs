mod runs;

use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use include_dir::{Dir, include_dir};
use serde_json::json;

pub use runs::{
    BODY, CALLS, CHECK, CHECKS, CODE, DIAGRAM, FAILED, FILE, FROM, HOPS, INPUT, MACHINE, METHOD,
    ORDER, PATH, REFUSED, REPLY, Runs, STATE, STATUS, STEP, STEPS, TEST_CASE, TEST_CASES, TO,
    processor,
};

static SITE: Dir = include_dir!("$CARGO_MANIFEST_DIR/site");
const IDLE: Duration = Duration::from_secs(5);
const TAKEOVER: Duration = Duration::from_secs(5);
const IDLE_BIND: Duration = Duration::from_millis(100);
const KEPT: usize = 192 * 1024 * 1024;
const POST: &str = "POST";
const OPTIONS: &str = "OPTIONS";
const QUIT: &str = "quit";
const OK: &str = "200 OK";
const MISSING: &str = "404 Not Found";
const OPEN: &str = "access-control-allow-origin: *\r\n";
const EVENTS: &str = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-store\r\naccess-control-allow-origin: *\r\n\r\n";
const ALLOWED: &str = "HTTP/1.1 204 No Content\r\naccess-control-allow-origin: *\r\naccess-control-allow-methods: POST, GET\r\naccess-control-allow-headers: *\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";

pub struct Reply {
    pub status: &'static str,
    pub kind: &'static str,
    pub body: Cow<'static, [u8]>,
}

pub trait Board: Sync {
    const PAGE: &str;

    fn schema(&self) -> String;

    fn opened(&self) {}

    fn kept(&self) -> Vec<String> {
        Vec::new()
    }

    fn posted(&self, _path: &str, _kind: &str, _body: &[u8]) -> Option<Reply> {
        None
    }
}

#[derive(Default)]
struct Rows {
    run: u64,
    dropped: usize,
    bytes: usize,
    kept: VecDeque<String>,
}

struct Log {
    rows: Mutex<Rows>,
    grown: Condvar,
}

static LOG: Log = Log {
    rows: Mutex::new(Rows {
        run: 0,
        dropped: 0,
        bytes: 0,
        kept: VecDeque::new(),
    }),
    grown: Condvar::new(),
};

struct Request {
    method: String,
    path: String,
    kind: String,
    body: Vec<u8>,
}

impl Log {
    fn held(&self) -> MutexGuard<'_, Rows> {
        self.rows.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn stream(&self, mut to: TcpStream, kept: Vec<String>) -> io::Result<()> {
        to.write_all(EVENTS.as_bytes())?;
        let (run, mut sent) = {
            let log = self.held();
            (log.run, log.dropped)
        };
        let mut kept = Some(kept);
        loop {
            let new = {
                let mut log = self.held();
                if log.run == run && log.dropped + log.kept.len() == sent && kept.is_none() {
                    log = self
                        .grown
                        .wait_timeout(log, IDLE)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                }
                match log.run == run {
                    true => {
                        let from = sent.max(log.dropped);
                        sent = log.dropped + log.kept.len();
                        log.kept.range(from - log.dropped..).cloned().collect()
                    }
                    false => vec![json!({ "reload": true }).to_string()],
                }
            };
            for rows in new.iter().chain(kept.take().iter().flatten()) {
                write!(to, "data: {rows}\n\n")?;
            }
            to.write_all(b": \n\n")?;
            to.flush()?;
        }
    }
}

pub fn say(rows: String) {
    let mut log = LOG.held();
    log.bytes += rows.len();
    log.kept.push_back(rows);
    while log.bytes > KEPT {
        let Some(oldest) = log.kept.pop_front() else {
            break;
        };
        log.bytes -= oldest.len();
        log.dropped += 1;
    }
    drop(log);
    LOG.grown.notify_all();
}

pub fn clear() {
    let mut log = LOG.held();
    *log = Rows {
        run: log.run + 1,
        ..Rows::default()
    };
    LOG.grown.notify_all();
}

fn kind(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript",
        Some("css") => "text/css",
        Some("wasm") => "application/wasm",
        Some("json") => "application/json",
        _ => "text/plain; charset=utf-8",
    }
}

fn read(stream: &TcpStream) -> io::Result<Request> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts
        .next()
        .unwrap_or_default()
        .trim_start_matches('/')
        .to_string();
    let (mut length, mut kind) = (0, String::new());
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            match name.trim().to_ascii_lowercase().as_str() {
                "content-length" => length = value.trim().parse().unwrap_or(0),
                "content-type" => kind = value.trim().to_string(),
                _ => {}
            }
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Request {
        method,
        path,
        kind,
        body,
    })
}

fn respond<B: Board>(mut stream: TcpStream, board: &B) -> io::Result<()> {
    let request = read(&stream)?;
    let (method, path) = (request.method.as_str(), request.path.as_str());
    if method == OPTIONS {
        return stream.write_all(ALLOWED.as_bytes());
    }
    if (method, path) == (POST, QUIT) {
        stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")?;
        std::process::exit(0);
    }
    let reply = match (method, path) {
        (POST, _) => board
            .posted(path, &request.kind, &request.body)
            .unwrap_or(Reply {
                status: MISSING,
                kind: kind(path),
                body: Cow::Borrowed(&[]),
            }),
        (_, "events") => return LOG.stream(stream, board.kept()),
        (_, "schema") => Reply {
            status: OK,
            kind: kind("schema.json"),
            body: Cow::Owned(board.schema().into_bytes()),
        },
        (_, "") => {
            board.opened();
            Reply {
                status: OK,
                kind: kind(B::PAGE),
                body: Cow::Borrowed(SITE.get_file(B::PAGE).map_or(&[], |f| f.contents())),
            }
        }
        _ => match SITE.get_file(path) {
            Some(file) => Reply {
                status: OK,
                kind: kind(path),
                body: Cow::Borrowed(file.contents()),
            },
            None => Reply {
                status: MISSING,
                kind: kind(path),
                body: Cow::Borrowed(&[]),
            },
        },
    };
    write!(
        stream,
        "HTTP/1.1 {}\r\ncontent-type: {}\r\ncache-control: no-store\r\n{OPEN}content-length: {}\r\nconnection: close\r\n\r\n",
        reply.status,
        reply.kind,
        reply.body.len()
    )?;
    stream.write_all(&reply.body)
}

fn bound(address: &str) -> Result<TcpListener, String> {
    let mut asked = false;
    let until = Instant::now() + TAKEOVER;
    loop {
        match TcpListener::bind(address) {
            Ok(listener) => return Ok(listener),
            Err(e) if e.kind() == io::ErrorKind::AddrInUse && Instant::now() < until => {
                if !asked {
                    asked = true;
                    if let Ok(mut old) = TcpStream::connect(address) {
                        let _ = write!(old, "{POST} /{QUIT} HTTP/1.1\r\nhost: {address}\r\n\r\n");
                    }
                }
                thread::sleep(IDLE_BIND);
            }
            Err(e) => return Err(format!("{address}: {e}")),
        }
    }
}

pub fn serve(address: &str, board: &impl Board) -> Result<(), String> {
    let listener = bound(address)?;
    println!("Weft: http://{address}");
    board.opened();
    thread::scope(|s| {
        for stream in listener.incoming().flatten() {
            s.spawn(move || {
                let _ = respond(stream, board);
            });
        }
    });
    Ok(())
}
