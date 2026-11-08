#![allow(non_camel_case_types, clippy::missing_safety_doc)]

use std::ffi::c_char;
use std::ptr;

use weft_draw::Theme;
use weft_parse::{Diagram, Verdict};
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
}

use weft_result_t::*;
#[repr(C)]
pub enum weft_format_t {
    WEFT_FORMAT_SVG_LIGHT = 0,
    WEFT_FORMAT_SVG_DARK = 1,
    WEFT_FORMAT_ASCII = 2,
}
#[repr(C)]
pub enum weft_status_t {
    WEFT_STATUS_NO_CHECK = 0,
    WEFT_STATUS_HOLDS = 1,
    WEFT_STATUS_FAILS = 2,
}
#[repr(C)]
pub struct weft_verdict_t {
    pub status: u32,
    pub step: usize,
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

unsafe fn diagram(
    ctx: *mut WeftContext,
    call: &str,
    source: weft_string_t,
) -> Result<Diagram, weft_result_t> {
    set_error(ctx, "");
    if source.data.is_null() && source.len != 0 {
        set_error(ctx, &format!("{call}: source data is NULL"));
        return Err(WEFT_E_INVALID_ARG);
    }
    let bytes = match source.len {
        0 => &[][..],
        len => unsafe { std::slice::from_raw_parts(source.data.cast::<u8>(), len) },
    };
    let text = std::str::from_utf8(bytes).map_err(|_| {
        set_error(ctx, &format!("{call}: source is not UTF-8"));
        WEFT_E_INVALID_ARG
    })?;
    weft_parse::parse(text).map_err(|e| {
        set_error(ctx, &e.to_string());
        WEFT_E_PARSE
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn weft_render(
    ctx: *mut WeftContext,
    source: weft_string_t,
    format: u32,
    out_result: *mut weft_owned_string_t,
) -> weft_result_t {
    let d = match unsafe { diagram(ctx, "weft_render", source) } {
        Ok(d) => d,
        Err(rc) => return rc,
    };
    let drawn = match format {
        f if f == weft_format_t::WEFT_FORMAT_SVG_LIGHT as u32 => weft_draw::svg(&d, Theme::Light),
        f if f == weft_format_t::WEFT_FORMAT_SVG_DARK as u32 => weft_draw::svg(&d, Theme::Dark),
        f if f == weft_format_t::WEFT_FORMAT_ASCII as u32 => weft_draw::ascii(&d),
        _ => {
            set_error(ctx, "weft_render: format is not a weft_format_t");
            return WEFT_E_INVALID_ARG;
        }
    };
    let Some(out) = (unsafe { out_result.as_mut() }) else {
        set_error(ctx, "weft_render: out_result is NULL");
        return WEFT_E_INVALID_ARG;
    };
    let mut bytes = match drawn {
        Ok(drawn) => drawn.into_bytes(),
        Err(e) => {
            set_error(ctx, &e);
            return WEFT_E_RENDER;
        }
    };
    let len = bytes.len();
    bytes.push(0);
    *out = weft_string_t {
        data: Box::into_raw(bytes.into_boxed_slice()).cast::<c_char>(),
        len,
    };
    WEFT_OK
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn weft_check(
    ctx: *mut WeftContext,
    source: weft_string_t,
    out_verdict: *mut weft_verdict_t,
) -> weft_result_t {
    let d = match unsafe { diagram(ctx, "weft_check", source) } {
        Ok(d) => d,
        Err(rc) => return rc,
    };
    let Some(out) = (unsafe { out_verdict.as_mut() }) else {
        set_error(ctx, "weft_check: out_verdict is NULL");
        return WEFT_E_INVALID_ARG;
    };
    *out = match d.verdict() {
        None => weft_verdict_t {
            status: weft_status_t::WEFT_STATUS_NO_CHECK as u32,
            step: 0,
        },
        Some(Verdict::Holds) => weft_verdict_t {
            status: weft_status_t::WEFT_STATUS_HOLDS as u32,
            step: 0,
        },
        Some(Verdict::Fails { step, .. }) => weft_verdict_t {
            status: weft_status_t::WEFT_STATUS_FAILS as u32,
            step,
        },
    };
    WEFT_OK
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
