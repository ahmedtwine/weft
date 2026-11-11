use std::env;
use std::os::unix::net::UnixStream;
use std::process;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use weft_wire::{Reader, Short, Writer, receive, send};

const AT: &str = "WEFT_TRANSPORT";
const UNIX: &str = "unix:";
const CUT: &str = "the reply of weft is too short";

enum Link {
    Unix(UnixStream),
    Http(ureq::Agent, String),
}

static LINK: Mutex<Option<Link>> = Mutex::new(None);

fn held<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(|p| p.into_inner())
}

fn connect() -> Result<Link, String> {
    let at = env::var(AT).map_err(|_| format!("{AT} is not set"))?;
    match at.strip_prefix(UNIX) {
        Some(path) => UnixStream::connect(path)
            .map(Link::Unix)
            .map_err(|e| format!("{path}: {e}")),
        None => Ok(Link::Http(
            ureq::Agent::new_with_defaults(),
            format!(
                "{}/{:x}{:x}",
                at.trim_end_matches('/'),
                process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos())
            ),
        )),
    }
}

fn ask(message: &[u8]) -> Result<Vec<u8>, String> {
    let mut link = held(&LINK);
    if link.is_none() {
        *link = Some(connect()?);
    }
    match link.as_mut() {
        Some(Link::Unix(stream)) => send(stream, message)
            .and_then(|()| receive(stream))
            .map_err(|e| format!("weft: {e}")),
        Some(Link::Http(agent, url)) => agent
            .post(url.as_str())
            .send(message)
            .and_then(|mut reply| reply.body_mut().read_to_vec())
            .map_err(|e| format!("POST {url}: {e}")),
        None => Err(format!("{AT} is not set")),
    }
}

pub fn said(call: u8, part: &[u8]) -> Result<String, String> {
    let cut = |Short| CUT.to_string();
    let reply = ask(&Writer::call(call, part).0)?;
    let mut r = Reader(&reply);
    let code = r.code().map_err(cut)?;
    let text = String::from_utf8_lossy(r.bytes().map_err(cut)?).into_owned();
    match code {
        0 => Ok(text),
        _ => Err(text),
    }
}
