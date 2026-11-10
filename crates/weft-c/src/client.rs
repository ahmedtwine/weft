use std::collections::HashMap;
use std::env;
use std::ffi::{c_char, c_void};
use std::os::unix::net::UnixStream;
use std::process;
use std::ptr;
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use weft_wire::{Reader, Short, Writer, receive, send};

const AT: &str = "WEFT_TRANSPORT";
const UNIX: &str = "unix:";
const INVALID: i32 = -5;
const CUT: &str = "the reply of weft is too short";

pub struct Context {
    last: Box<[u8]>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Text {
    pub data: *const c_char,
    pub len: usize,
}

pub type Callback = Option<unsafe extern "C" fn(*mut c_void, *const c_char, usize)>;

enum Link {
    Unix(UnixStream),
    Http(ureq::Agent, String),
}

struct Kept {
    _text: Box<Text>,
    _bytes: Box<[u8]>,
}

unsafe impl Send for Kept {}

static LINK: Mutex<Option<Link>> = Mutex::new(None);
static KEPT: LazyLock<Mutex<HashMap<u64, Vec<Kept>>>> = LazyLock::new(Mutex::default);
static PURE: LazyLock<Mutex<HashMap<Vec<u8>, Vec<u8>>>> = LazyLock::new(Mutex::default);
static PENDING: Mutex<(u32, Vec<u8>)> = Mutex::new((0, Vec::new()));

fn held<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(|p| p.into_inner())
}

fn ended(bytes: &[u8]) -> Box<[u8]> {
    [bytes, &[0]].concat().into_boxed_slice()
}

impl Default for Context {
    fn default() -> Context {
        Context { last: ended(&[]) }
    }
}

impl Text {
    pub unsafe fn bytes<'a>(self) -> &'a [u8] {
        match self.data.is_null() {
            true => &[],
            false => unsafe { std::slice::from_raw_parts(self.data.cast::<u8>(), self.len) },
        }
    }
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

fn replied(calls: &Writer) -> Result<Vec<u8>, (i32, String)> {
    let cut = |Short| (INVALID, CUT.to_string());
    let mut message = Writer::default();
    message.put(PRINT);
    {
        let mut pending = held(&PENDING);
        message.put(pending.0 + 1);
        message.0.append(&mut pending.1);
        pending.0 = 0;
    }
    message.0.extend_from_slice(&calls.0);
    let reply = ask(&message.0).map_err(|e| (INVALID, e))?;
    let mut r = Reader(&reply);
    match r.get::<i32>().map_err(cut)? {
        0 => Ok(r.0.to_vec()),
        code => Err((
            code,
            String::from_utf8_lossy(r.bytes().map_err(cut)?).into_owned(),
        )),
    }
}

pub fn said(call: u16, parts: &[&[u8]]) -> Result<Vec<u8>, String> {
    let mut w = self::call(call);
    parts.iter().for_each(|part| w.bytes(part));
    replied(&w).map_err(|(_, e)| e)
}

fn call(index: u16) -> Writer {
    let mut w = Writer::default();
    w.put(index);
    w
}

unsafe fn deferred(_ctx: *mut Context, w: Writer) -> i32 {
    let mut pending = held(&PENDING);
    pending.0 += 1;
    pending.1.extend_from_slice(&w.0);
    0
}

unsafe fn told(
    ctx: *mut Context,
    reply: Result<Vec<u8>, (i32, String)>,
    read: impl FnOnce(&mut Reader) -> Result<(), Short>,
) -> i32 {
    let told =
        reply.and_then(|outs| read(&mut Reader(&outs)).map_err(|Short| (INVALID, CUT.to_string())));
    let (code, message) = told.err().unwrap_or_default();
    if let Some(ctx) = unsafe { ctx.as_mut() } {
        ctx.last = ended(message.as_bytes());
    }
    code
}

unsafe fn asked(
    ctx: *mut Context,
    w: Writer,
    read: impl FnOnce(&mut Reader) -> Result<(), Short>,
) -> i32 {
    unsafe { told(ctx, replied(&w), read) }
}

unsafe fn pure(
    ctx: *mut Context,
    w: Writer,
    read: impl FnOnce(&mut Reader) -> Result<(), Short>,
) -> i32 {
    let known = held(&PURE).get(&w.0).cloned();
    let reply = match known {
        Some(outs) => Ok(outs),
        None => replied(&w).inspect(|outs| {
            held(&PURE).insert(w.0.clone(), outs.clone());
        }),
    };
    unsafe { told(ctx, reply, read) }
}

unsafe fn freed(ctx: *mut Context) -> i32 {
    if !ctx.is_null() {
        drop(unsafe { Box::from_raw(ctx) });
    }
    0
}

unsafe fn last(ctx: *const Context) -> Text {
    match unsafe { ctx.as_ref() } {
        Some(c) => Text {
            data: c.last.as_ptr().cast(),
            len: c.last.len() - 1,
        },
        None => Text {
            data: c"".as_ptr(),
            len: 0,
        },
    }
}

unsafe fn out<T>(to: *mut T, value: T) {
    if !to.is_null() {
        unsafe { to.write(value) };
    }
}

unsafe fn many<'a, T>(data: *const T, len: usize) -> Option<&'a [T]> {
    (!data.is_null()).then(|| unsafe { std::slice::from_raw_parts(data, len) })
}

unsafe fn copied(to: *mut u8, most: usize, bytes: &[u8]) {
    if !to.is_null() {
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), to, bytes.len().min(most)) };
    }
}

fn lent(owner: u64, bytes: &[u8]) -> *const Text {
    let bytes = ended(bytes);
    let text = Box::new(Text {
        data: bytes.as_ptr().cast(),
        len: bytes.len() - 1,
    });
    let at = &raw const *text;
    held(&KEPT).entry(owner).or_default().push(Kept {
        _text: text,
        _bytes: bytes,
    });
    at
}

fn kept(owner: u64, bytes: &[u8]) -> Text {
    unsafe { *lent(owner, bytes) }
}

fn released(owner: u64) {
    held(&KEPT).remove(&owner);
}

fn given(bytes: &[u8]) -> Text {
    Text {
        data: Box::into_raw(ended(bytes)).cast::<c_char>(),
        len: bytes.len(),
    }
}

unsafe fn dropped(result: *mut Text) -> i32 {
    if let Some(r) = unsafe { result.as_mut() } {
        if !r.data.is_null() {
            drop(unsafe {
                Box::from_raw(ptr::slice_from_raw_parts_mut(
                    r.data.cast_mut().cast::<u8>(),
                    r.len + 1,
                ))
            });
        }
        *r = Text {
            data: ptr::null(),
            len: 0,
        };
    }
    0
}

include!(concat!(env!("OUT_DIR"), "/hegel.rs"));
