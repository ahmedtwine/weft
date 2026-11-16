#![allow(non_camel_case_types, clippy::missing_safety_doc)]

use std::ffi::c_char;
use std::path::Path;
use std::ptr;
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use serde_json::{Value as Json, json};
use weft_draw::{Code, Theme};
use weft_orchestrator::{Event, checks, summary, totals};
use weft_parse::Diagram;
use weft_wire::{
    ASSUME, DONE, GENERATE, JOIN, POOL_ADD, POOL_GENERATE, STOP_TEST, TARGET, TURN, client,
};
#[repr(C)]
#[derive(Clone, Copy)]
pub struct weft_string_t {
    pub data: *const c_char,
    pub len: usize,
}
pub type weft_owned_string_t = weft_string_t;
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum weft_result_t {
    WEFT_OK = 0,
    WEFT_E_PARSE = -1,
    WEFT_E_INVALID_ARG = -2,
    WEFT_E_RENDER = -3,
    WEFT_E_REFUSED = -4,
    WEFT_E_STOP_TEST = -5,
    WEFT_E_ASSUME = -6,
    WEFT_E_FAILED = -7,
}

use weft_result_t::*;
#[repr(C)]
pub enum weft_format_t {
    WEFT_FORMAT_SVG_LIGHT = 0,
    WEFT_FORMAT_SVG_DARK = 1,
    WEFT_FORMAT_ASCII = 2,
}
pub struct WeftContext {
    last_error: Box<[u8]>,
}

fn terminated(s: &str) -> Box<[u8]> {
    let mut bytes = s.as_bytes().to_vec();
    bytes.push(0);
    bytes.into_boxed_slice()
}

fn set_error(ctx: *mut WeftContext, message: &str) {
    if let Some(c) = unsafe { ctx.as_mut() } {
        c.last_error = terminated(message);
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn weft_context_new() -> *mut WeftContext {
    Box::into_raw(Box::new(WeftContext {
        last_error: terminated(""),
    }))
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn weft_context_free(ctx: *mut WeftContext) -> weft_result_t {
    if !ctx.is_null() {
        drop(unsafe { Box::from_raw(ctx) });
    }
    WEFT_OK
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn weft_context_last_error(ctx: *const WeftContext) -> weft_string_t {
    match unsafe { ctx.as_ref() } {
        Some(c) => weft_string_t {
            data: c.last_error.as_ptr().cast(),
            len: c.last_error.len() - 1,
        },
        None => weft_string_t {
            data: c"".as_ptr(),
            len: 0,
        },
    }
}

unsafe fn text<'a>(
    ctx: *mut WeftContext,
    call: &str,
    what: &str,
    s: weft_string_t,
) -> Result<&'a str, weft_result_t> {
    if s.data.is_null() && s.len != 0 {
        set_error(ctx, &format!("{call}: {what} data is NULL"));
        return Err(WEFT_E_INVALID_ARG);
    }
    let bytes = match s.len {
        0 => &[][..],
        len => unsafe { std::slice::from_raw_parts(s.data.cast::<u8>(), len) },
    };
    std::str::from_utf8(bytes).map_err(|_| {
        set_error(ctx, &format!("{call}: {what} is not UTF-8"));
        WEFT_E_INVALID_ARG
    })
}

unsafe fn diagram(
    ctx: *mut WeftContext,
    call: &str,
    config: weft_string_t,
    source: weft_string_t,
) -> Result<Diagram, weft_result_t> {
    set_error(ctx, "");
    let config =
        weft_parse::config(unsafe { text(ctx, call, "config", config) }?).map_err(|e| {
            set_error(ctx, &format!("{call}: config: {e}"));
            WEFT_E_PARSE
        })?;
    weft_parse::parse(unsafe { text(ctx, call, "source", source) }?, &config).map_err(|e| {
        set_error(ctx, &e.to_string());
        WEFT_E_PARSE
    })
}
unsafe fn give(
    ctx: *mut WeftContext,
    call: &str,
    out_result: *mut weft_owned_string_t,
    text: String,
) -> weft_result_t {
    let Some(out) = (unsafe { out_result.as_mut() }) else {
        set_error(ctx, &format!("{call}: out_result is NULL"));
        return WEFT_E_INVALID_ARG;
    };
    let mut bytes = text.into_bytes();
    let len = bytes.len();
    bytes.push(0);
    *out = weft_string_t {
        data: Box::into_raw(bytes.into_boxed_slice()).cast::<c_char>(),
        len,
    };
    WEFT_OK
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn weft_render(
    ctx: *mut WeftContext,
    config: weft_string_t,
    source: weft_string_t,
    format: u32,
    out_result: *mut weft_owned_string_t,
) -> weft_result_t {
    let d = match unsafe { diagram(ctx, "weft_render", config, source) } {
        Ok(d) => d,
        Err(rc) => return rc,
    };
    let drawn = match format {
        f if f == weft_format_t::WEFT_FORMAT_SVG_LIGHT as u32 => {
            weft_draw::svg(&d, Some(Theme::Light), &Code::default())
        }
        f if f == weft_format_t::WEFT_FORMAT_SVG_DARK as u32 => {
            weft_draw::svg(&d, Some(Theme::Dark), &Code::default())
        }
        f if f == weft_format_t::WEFT_FORMAT_ASCII as u32 => weft_draw::ascii(&d),
        _ => {
            set_error(ctx, "weft_render: format is not a weft_format_t");
            return WEFT_E_INVALID_ARG;
        }
    };
    match drawn {
        Ok(drawn) => unsafe { give(ctx, "weft_render", out_result, drawn) },
        Err(e) => {
            set_error(ctx, &e);
            WEFT_E_RENDER
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn weft_owned_string_free(
    ctx: *mut WeftContext,
    result: *mut weft_owned_string_t,
) -> weft_result_t {
    set_error(ctx, "");
    let Some(r) = (unsafe { result.as_mut() }) else {
        return WEFT_OK;
    };
    if !r.data.is_null() {
        drop(unsafe {
            Box::from_raw(ptr::slice_from_raw_parts_mut(
                r.data.cast_mut().cast::<u8>(),
                r.len + 1,
            ))
        });
    }
    *r = weft_string_t {
        data: ptr::null(),
        len: 0,
    };
    WEFT_OK
}

unsafe fn sent(
    ctx: *mut WeftContext,
    call: &str,
    index: u8,
    part: weft_string_t,
) -> Result<String, weft_result_t> {
    set_error(ctx, "");
    let part = unsafe { text(ctx, call, "the message", part) }?;
    client::said(index, part.as_bytes()).map_err(|(code, e)| {
        set_error(ctx, &e);
        match code {
            STOP_TEST => WEFT_E_STOP_TEST,
            ASSUME => WEFT_E_ASSUME,
            _ => WEFT_E_REFUSED,
        }
    })
}

fn given(said: &str) -> weft_string_t {
    weft_string_t {
        data: said.as_ptr().cast(),
        len: said.len(),
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn weft_diagram_join(
    ctx: *mut WeftContext,
    name: weft_string_t,
    out_code: *mut weft_owned_string_t,
) -> weft_result_t {
    let call = "weft_diagram_join";
    match unsafe { sent(ctx, call, JOIN, name) } {
        Ok(code) => unsafe { give(ctx, call, out_code, code) },
        Err(rc) => rc,
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn weft_diagram_turn(
    ctx: *mut WeftContext,
    report: weft_string_t,
    out_turn: *mut weft_owned_string_t,
) -> weft_result_t {
    let call = "weft_diagram_turn";
    match unsafe { sent(ctx, call, TURN, report) } {
        Ok(turn) => unsafe { give(ctx, call, out_turn, turn) },
        Err(rc) => rc,
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn weft_diagram_done(
    ctx: *mut WeftContext,
    said: weft_string_t,
) -> weft_result_t {
    match unsafe { sent(ctx, "weft_diagram_done", DONE, said) } {
        Ok(_) => WEFT_OK,
        Err(rc) => rc,
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn weft_generate(
    ctx: *mut WeftContext,
    schema: weft_string_t,
    out_value: *mut weft_owned_string_t,
) -> weft_result_t {
    let call = "weft_generate";
    match unsafe { sent(ctx, call, GENERATE, schema) } {
        Ok(value) => unsafe { give(ctx, call, out_value, value) },
        Err(rc) => rc,
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn weft_pool_add(
    ctx: *mut WeftContext,
    pool: weft_string_t,
    value: weft_string_t,
) -> weft_result_t {
    let call = "weft_pool_add";
    let said = unsafe { text(ctx, call, "pool", pool) }.and_then(|pool| {
        let value = unsafe { text(ctx, call, "value", value) }?;
        let value: Json = serde_json::from_str(value).map_err(|e| {
            set_error(ctx, &format!("{call}: value: {e}"));
            WEFT_E_INVALID_ARG
        })?;
        Ok(json!({ "pool": pool, "value": value }).to_string())
    });
    match said.and_then(|said| unsafe { sent(ctx, call, POOL_ADD, given(&said)) }) {
        Ok(_) => WEFT_OK,
        Err(rc) => rc,
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn weft_pool_generate(
    ctx: *mut WeftContext,
    pool: weft_string_t,
    consume: bool,
    out_value: *mut weft_owned_string_t,
) -> weft_result_t {
    let call = "weft_pool_generate";
    let said = unsafe { text(ctx, call, "pool", pool) }
        .map(|pool| json!({ "pool": pool, "consume": consume }).to_string());
    match said.and_then(|said| unsafe { sent(ctx, call, POOL_GENERATE, given(&said)) }) {
        Ok(value) => unsafe { give(ctx, call, out_value, value) },
        Err(rc) => rc,
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn weft_target(
    ctx: *mut WeftContext,
    value: f64,
    label: weft_string_t,
) -> weft_result_t {
    let call = "weft_target";
    let said = unsafe { text(ctx, call, "label", label) }
        .map(|label| json!({ "value": value, "label": label }).to_string());
    match said.and_then(|said| unsafe { sent(ctx, call, TARGET, given(&said)) }) {
        Ok(_) => WEFT_OK,
        Err(rc) => rc,
    }
}

type Test = JoinHandle<Result<(bool, String), String>>;

static TEST: Mutex<Option<Test>> = Mutex::new(None);

fn tested(adapter: &str, file: &str, joined: &Sender<String>) -> Result<(bool, String), String> {
    weft_orchestrator::rooted()?;
    let config = weft_orchestrator::config()?;
    let path = Path::new(file);
    let d = weft_orchestrator::diagram(path, &config)?;
    let started = Instant::now();
    let o = weft_orchestrator::tested(&config, &d, path, Some(adapter), &mut |event| {
        if let Event::Joined(line) = event {
            let _ = joined.send(line.to_string());
        }
    })?;
    let line = summary(file, &d, &config, &o, started.elapsed());
    Ok((o.failed, totals(&[line], checks(&o), started.elapsed())))
}

fn ended(test: Test) -> String {
    test.join().map_or_else(
        |_| "the test panics".into(),
        |r| r.err().unwrap_or_default(),
    )
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn weft_test_start(
    ctx: *mut WeftContext,
    adapter: weft_string_t,
    file: weft_string_t,
    out_joined: *mut weft_owned_string_t,
) -> weft_result_t {
    let call = "weft_test_start";
    set_error(ctx, "");
    let adapter = match unsafe { text(ctx, call, "adapter", adapter) } {
        Ok(adapter) => adapter.to_string(),
        Err(rc) => return rc,
    };
    let file = match unsafe { text(ctx, call, "file", file) } {
        Ok(file) => file.to_string(),
        Err(rc) => return rc,
    };
    let (sender, joined) = mpsc::channel();
    let test = thread::spawn(move || tested(&adapter, &file, &sender));
    match joined.recv() {
        Ok(line) => {
            *TEST.lock().unwrap_or_else(PoisonError::into_inner) = Some(test);
            unsafe { give(ctx, call, out_joined, line) }
        }
        Err(_) => {
            set_error(ctx, &ended(test));
            WEFT_E_REFUSED
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn weft_test_end(
    ctx: *mut WeftContext,
    out_report: *mut weft_owned_string_t,
) -> weft_result_t {
    let call = "weft_test_end";
    set_error(ctx, "");
    let Some(test) = TEST.lock().unwrap_or_else(PoisonError::into_inner).take() else {
        set_error(ctx, &format!("{call}: no test runs"));
        return WEFT_E_INVALID_ARG;
    };
    match test.join() {
        Ok(Ok((failed, report))) => match unsafe { give(ctx, call, out_report, report) } {
            WEFT_OK if failed => WEFT_E_FAILED,
            rc => rc,
        },
        Ok(Err(e)) => {
            set_error(ctx, &e);
            WEFT_E_REFUSED
        }
        Err(_) => {
            set_error(ctx, &format!("{call}: the test panics"));
            WEFT_E_REFUSED
        }
    }
}
