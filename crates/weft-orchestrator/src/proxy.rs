use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crate::hub::Session;
use crate::simulate::Hop;

const WAIT: Duration = Duration::from_secs(60);
const IDLE: Duration = Duration::from_millis(20);
const SKIPPED: [&str; 4] = ["host", "content-length", "connection", "transfer-encoding"];

pub(crate) struct Proxy {
    pub url: String,
    closed: Arc<AtomicBool>,
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Relaxed);
    }
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn request(reader: &mut BufReader<TcpStream>) -> Option<Request> {
    let mut line = String::new();
    reader.read_line(&mut line).ok().filter(|n| *n > 0)?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next()?.to_string(), parts.next()?.to_string());
    let mut headers = Vec::new();
    let mut size = 0;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).ok().filter(|n| *n > 0)?;
        match header.trim().split_once(':') {
            Some((name, value)) => {
                let (name, value) = (name.trim().to_ascii_lowercase(), value.trim().to_string());
                if name == "content-length" {
                    size = value.parse().ok()?;
                }
                headers.push((name, value));
            }
            None => break,
        }
    }
    let mut body = vec![0; size];
    reader.read_exact(&mut body).ok()?;
    Some(Request {
        method,
        path,
        headers,
        body,
    })
}

fn forward(r: &Request, url: &str) -> Result<(u16, String, Vec<u8>), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    let mut built = ureq::http::Request::builder()
        .method(r.method.as_str())
        .uri(url);
    for (name, value) in r
        .headers
        .iter()
        .filter(|(n, _)| !SKIPPED.contains(&n.as_str()))
    {
        built = built.header(name, value);
    }
    let sent = built.body(r.body.clone()).map_err(|e| e.to_string())?;
    let mut response = agent.run(sent).map_err(|e| format!("{url}: {e}"))?;
    let kind = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let body = response
        .body_mut()
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    Ok((response.status().as_u16(), kind, body))
}

fn serve(stream: TcpStream, from: &str, session: &Session, routes: &[(String, String)]) {
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    while let Some(r) = request(&mut reader) {
        let sent = session.tick();
        let (to, rest) = match r.path.trim_start_matches('/').split_once('/') {
            Some((to, rest)) => (to.to_string(), format!("/{rest}")),
            None => (r.path.trim_start_matches('/').to_string(), "/".to_string()),
        };
        let adapter = routes
            .iter()
            .find(|(app, _)| *app == to)
            .map_or(to.as_str(), |(_, adapter)| adapter.as_str());
        let (status, kind, body) = session
            .serves(adapter, WAIT)
            .and_then(|base| forward(&r, &format!("{}{rest}", base.trim_end_matches('/'))))
            .unwrap_or_else(|e| (502, "text/plain".into(), e.into_bytes()));
        session.record(Hop {
            from: from.to_string(),
            to,
            method: r.method.clone(),
            path: rest,
            body: String::from_utf8_lossy(&r.body).into_owned(),
            status,
            reply: String::from_utf8_lossy(&body).into_owned(),
            sent,
            answered: session.tick(),
        });
        let head = format!(
            "HTTP/1.1 {status} weft\r\ncontent-type: {kind}\r\ncontent-length: {}\r\n\r\n",
            body.len()
        );
        if writer
            .write_all(head.as_bytes())
            .and_then(|()| writer.write_all(&body))
            .is_err()
        {
            break;
        }
    }
}

pub(crate) type Routes = Arc<Vec<(String, String)>>;

pub(crate) fn open(from: &str, session: Arc<Session>, routes: Routes) -> Result<Proxy, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let url = format!(
        "http://{}",
        listener.local_addr().map_err(|e| e.to_string())?
    );
    let closed = Arc::new(AtomicBool::new(false));
    let (shut, from) = (closed.clone(), from.to_string());
    thread::spawn(move || {
        while !shut.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let (from, session, routes) = (from.clone(), session.clone(), routes.clone());
                    thread::spawn(move || serve(stream, &from, &session, &routes));
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => thread::sleep(IDLE),
                Err(_) => break,
            }
        }
    });
    Ok(Proxy { url, closed })
}
