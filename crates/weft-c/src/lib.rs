#![allow(non_camel_case_types)]

use std::ffi::c_char;
use std::ptr;

use weft_parse::{Diagram, Verdict};

/// A UTF-8 string with an explicit length.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct weft_string_t {
    pub data: *const c_char,
    pub len: usize,
}

/// A string that libweft allocated. Release it with `weft_owned_string_free`.
pub type weft_owned_string_t = weft_string_t;

/// The result of a libweft call.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum weft_result_t {
    /// Success.
    WEFT_OK = 0,
    /// The source does not parse. See `weft_context_last_error`.
    WEFT_E_PARSE = -1,
    /// An argument was not valid.
    WEFT_E_INVALID_ARG = -2,
    /// merman could not draw the diagram. See `weft_context_last_error`.
    WEFT_E_RENDER = -3,
}

use weft_result_t::*;

/// The output format of `weft_render`, passed as a uint32_t.
#[repr(C)]
pub enum weft_format_t {
    WEFT_FORMAT_SVG = 0,
    WEFT_FORMAT_ASCII = 1,
}

/// The status of a `weft_verdict_t`.
#[repr(C)]
pub enum weft_status_t {
    /// The diagram has no check.
    WEFT_STATUS_NO_CHECK = 0,
    /// The run does not break the check.
    WEFT_STATUS_HOLDS = 1,
    /// The run breaks the check at `step`.
    WEFT_STATUS_FAILS = 2,
}

/// The result of `weft_check`. `status` is a `weft_status_t`. `step` is the
/// zero-based index of the step that breaks the check.
#[repr(C)]
pub struct weft_verdict_t {
    pub status: u32,
    pub step: usize,
}

/// Holds the message of the last failed call.
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

/// Returns a new context. Never returns NULL. Release it with
/// `weft_context_free`.
#[unsafe(no_mangle)]
pub extern "C" fn weft_context_new() -> *mut WeftContext {
    Box::into_raw(Box::new(WeftContext {
        last_error: terminated(""),
    }))
}

/// Releases `ctx`. NULL is a no-op.
///
/// # Safety
/// `ctx` is NULL or a context from `weft_context_new` that is not released.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn weft_context_free(ctx: *mut WeftContext) -> weft_result_t {
    if !ctx.is_null() {
        drop(unsafe { Box::from_raw(ctx) });
    }
    WEFT_OK
}

/// Returns the message of the last failed call on `ctx`, or the empty string.
///
/// # Safety
/// `ctx` is NULL or a live context.
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

/// Draws `source` in `format`, a `weft_format_t`, into `out_result`.
///
/// # Safety
/// `ctx` is NULL or a live context. `source` points to `len` readable bytes.
/// `out_result` points to writable memory.
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
    let draw: fn(&Diagram) -> Result<String, String> = match format {
        0 => weft_draw::svg,
        1 => weft_draw::ascii,
        _ => {
            set_error(ctx, "weft_render: format is not a weft_format_t");
            return WEFT_E_INVALID_ARG;
        }
    };
    let Some(out) = (unsafe { out_result.as_mut() }) else {
        set_error(ctx, "weft_render: out_result is NULL");
        return WEFT_E_INVALID_ARG;
    };
    let mut bytes = match draw(&d) {
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

/// Runs the steps of `source` and checks its invariant after each step.
///
/// # Safety
/// `ctx` is NULL or a live context. `source` points to `len` readable bytes.
/// `out_verdict` points to writable memory.
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

/// Releases a string from `weft_render` and resets it to `{NULL, 0}`. NULL
/// and a reset string are no-ops.
///
/// # Safety
/// `result` is NULL or a string from `weft_render` that is not released.
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
