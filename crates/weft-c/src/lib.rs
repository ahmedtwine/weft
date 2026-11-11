#![allow(non_camel_case_types, clippy::missing_safety_doc)]

mod client;

use std::ffi::c_char;
use std::ptr;

use weft_draw::{Code, Theme};
use weft_parse::Diagram;
use weft_wire::{DONE, JOIN, TURN};
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
            weft_draw::svg(&d, Theme::Light, &Code::default())
        }
        f if f == weft_format_t::WEFT_FORMAT_SVG_DARK as u32 => {
            weft_draw::svg(&d, Theme::Dark, &Code::default())
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
    client::said(index, part.as_bytes()).map_err(|e| {
        set_error(ctx, &e);
        WEFT_E_REFUSED
    })
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
