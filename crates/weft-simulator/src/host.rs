use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ptr;

use hegel_c::HegelContext;
use weft_wire::{Reader, Short, Writer};

use crate::engine::{bytes, raw, text};

const BUFFER: usize = 1 << 16;
const WIDTH: u64 = 16;

struct Held {
    kind: usize,
    at: *mut c_void,
    shared: bool,
}

pub struct Host {
    ctx: *mut HegelContext,
    held: HashMap<u64, Held>,
    last: u64,
}

unsafe impl Send for Host {}

pub fn kind(name: &str) -> usize {
    HANDLES
        .iter()
        .position(|h| *h == name)
        .unwrap_or(usize::MAX)
}

impl Default for Host {
    fn default() -> Host {
        Host {
            ctx: hegel_c::hegel_context_new(),
            held: HashMap::new(),
            last: 0,
        }
    }
}

impl Host {
    pub fn ctx(&self) -> *mut HegelContext {
        self.ctx
    }

    pub fn handle(&self, kind: usize, id: u64) -> Result<*mut c_void, Short> {
        match (id, self.held.get(&id)) {
            (0, _) => Ok(ptr::null_mut()),
            (_, Some(h)) if h.kind == kind => Ok(h.at),
            _ => Err(Short),
        }
    }

    fn held(&mut self, kind: usize, at: *mut c_void, shared: bool) -> u64 {
        if at.is_null() {
            return 0;
        }
        self.last += WIDTH;
        self.held.insert(self.last, Held { kind, at, shared });
        self.last
    }

    pub fn insert(&mut self, kind: usize, at: *mut c_void) -> u64 {
        self.held(kind, at, false)
    }

    pub fn share(&mut self, kind: usize, at: *mut c_void) -> u64 {
        self.held
            .iter()
            .find(|(_, h)| h.shared && h.kind == kind && h.at == at)
            .map(|(id, _)| *id)
            .unwrap_or_else(|| self.held(kind, at, true))
    }

    pub fn take(&mut self, kind: usize, id: u64) -> Result<Option<*mut c_void>, Short> {
        self.handle(kind, id)?;
        Ok(self.held.remove(&id).filter(|h| !h.shared).map(|h| h.at))
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        unsafe {
            let _ = hegel_c::hegel_context_free(self.ctx);
        }
    }
}

include!(concat!(env!("OUT_DIR"), "/hegel.rs"));
