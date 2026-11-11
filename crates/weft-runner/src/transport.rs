use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;

use weft_parse::Transport;
use weft_simulator::{Hub, Peer};
use weft_wire::{receive, send};

type Peers = Arc<Mutex<HashMap<String, Arc<Mutex<Peer>>>>>;

pub static HUB: LazyLock<Arc<Hub>> = LazyLock::new(Arc::default);
static OPEN: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Mutex::default);

fn unix(stream: UnixStream) {
    let mut peer = Peer::default();
    let mut stream = stream;
    while let Ok(message) = receive(&mut stream) {
        if send(&mut stream, &peer.serve(&HUB, &message)).is_err() {
            break;
        }
    }
}

fn request(reader: &mut BufReader<TcpStream>) -> Option<(String, Vec<u8>)> {
    let mut line = String::new();
    reader.read_line(&mut line).ok().filter(|n| *n > 0)?;
    let path = line.split(' ').nth(1)?.to_string();
    let mut size = 0;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).ok().filter(|n| *n > 0)?;
        match header.trim().split_once(':') {
            Some((name, value)) if name.eq_ignore_ascii_case("content-length") => {
                size = value.trim().parse().ok()?;
            }
            Some(_) => {}
            None => break,
        }
    }
    let mut body = vec![0; size];
    reader.read_exact(&mut body).ok()?;
    Some((path, body))
}

fn http(stream: TcpStream, peers: Peers) {
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    while let Some((path, body)) = request(&mut reader) {
        let peer = peers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry(path)
            .or_default()
            .clone();
        let reply = peer
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .serve(&HUB, &body);
        let head = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/octet-stream\r\ncontent-length: {}\r\n\r\n",
            reply.len()
        );
        if writer
            .write_all(head.as_bytes())
            .and_then(|()| writer.write_all(&reply))
            .is_err()
        {
            break;
        }
    }
}

fn open(transport: Option<&Transport>) -> Result<String, String> {
    let socket = |path: String| {
        let _ = fs::remove_file(&path);
        let listener = UnixListener::bind(&path).map_err(|e| format!("{path}: {e}"))?;
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                thread::spawn(move || unix(stream));
            }
        });
        Ok(format!("unix:{path}"))
    };
    match transport {
        None => socket(
            env::temp_dir()
                .join(format!("weft-{}.sock", std::process::id()))
                .display()
                .to_string(),
        ),
        Some(Transport::Unix(path)) => socket(path.clone()),
        Some(Transport::Http(url)) => {
            let host = url
                .strip_prefix("http://")
                .map(|rest| rest.split('/').next().unwrap_or_default())
                .ok_or_else(|| {
                    format!("`transport \"http\" url=\"{url}\"` has no `http://<host>:<port>`")
                })?;
            let listener = TcpListener::bind(host).map_err(|e| format!("{host}: {e}"))?;
            let peers = Peers::default();
            thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    let peers = peers.clone();
                    thread::spawn(move || http(stream, peers));
                }
            });
            Ok(url.trim_end_matches('/').to_string())
        }
    }
}

pub fn listen(transport: Option<&Transport>) -> Result<String, String> {
    let key = transport.map_or(String::new(), Transport::to_string);
    let mut all = OPEN.lock().unwrap_or_else(|p| p.into_inner());
    match all.get(&key) {
        Some(at) => Ok(at.clone()),
        None => {
            let at = open(transport)?;
            all.insert(key, at.clone());
            Ok(at)
        }
    }
}
