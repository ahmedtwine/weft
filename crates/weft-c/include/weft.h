/*
 * libweft — C bindings for weft, the invariant diagram language.
 *
 * Generated from crates/weft-c/src/lib.rs by cbindgen. Do not edit it.
 * Run `WEFT_C_HEADER_WRITE=1 cargo test -p weft-c` after a change.
 *
 * Calling convention
 * ------------------
 * Every function takes a weft_context_t* as its first argument and returns
 * a weft_result_t, except weft_context_new and weft_context_last_error.
 * WEFT_OK is zero and every error is negative. Outputs go through a trailing
 * out_* parameter. A NULL context opts out of error messages.
 *
 * Strings
 * -------
 * weft_string_t is UTF-8 with an explicit length. Inputs need no NUL.
 * Outputs have a trailing NUL that the length excludes.
 *
 * Ownership
 * ---------
 *     weft_context_new  ->  weft_context_free
 *     weft_render       ->  weft_owned_string_free
 *
 * weft_context_last_error is borrowed and valid until the next call on the
 * same context. A context must not be used from two threads at once.
 */

#ifndef WEFT_H
#define WEFT_H

#include <stddef.h>
#include <stdint.h>

/*
 The result of a libweft call.
 */
typedef enum {
  /*
   Success.
   */
  WEFT_OK = 0,
  /*
   The source does not parse. See `weft_context_last_error`.
   */
  WEFT_E_PARSE = -1,
  /*
   An argument was not valid.
   */
  WEFT_E_INVALID_ARG = -2,
  /*
   merman could not draw the diagram. See `weft_context_last_error`.
   */
  WEFT_E_RENDER = -3,
} weft_result_t;

/*
 The output format of `weft_render`, passed as a uint32_t.
 */
typedef enum {
  WEFT_FORMAT_SVG = 0,
  WEFT_FORMAT_ASCII = 1,
} weft_format_t;

/*
 The status of a `weft_verdict_t`.
 */
typedef enum {
  /*
   The diagram has no check.
   */
  WEFT_STATUS_NO_CHECK = 0,
  /*
   The run does not break the check.
   */
  WEFT_STATUS_HOLDS = 1,
  /*
   The run breaks the check at `step`.
   */
  WEFT_STATUS_FAILS = 2,
} weft_status_t;

/*
 Holds the message of the last failed call.
 */
typedef struct weft_context_t weft_context_t;

/*
 A UTF-8 string with an explicit length.
 */
typedef struct {
  const char *data;
  size_t len;
} weft_string_t;

/*
 A string that libweft allocated. Release it with `weft_owned_string_free`.
 */
typedef weft_string_t weft_owned_string_t;

/*
 The result of `weft_check`. `status` is a `weft_status_t`. `step` is the
 zero-based index of the step that breaks the check.
 */
typedef struct {
  uint32_t status;
  size_t step;
} weft_verdict_t;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/*
 Returns a new context. Never returns NULL. Release it with
 `weft_context_free`.
 */
weft_context_t *weft_context_new(void);

/*
 Releases `ctx`. NULL is a no-op.

 # Safety
 `ctx` is NULL or a context from `weft_context_new` that is not released.
 */
weft_result_t weft_context_free(weft_context_t *ctx);

/*
 Returns the message of the last failed call on `ctx`, or the empty string.

 # Safety
 `ctx` is NULL or a live context.
 */
weft_string_t weft_context_last_error(const weft_context_t *ctx);

/*
 Draws `source` in `format`, a `weft_format_t`, into `out_result`.

 # Safety
 `ctx` is NULL or a live context. `source` points to `len` readable bytes.
 `out_result` points to writable memory.
 */
weft_result_t weft_render(weft_context_t *ctx,
                          weft_string_t source,
                          uint32_t format,
                          weft_owned_string_t *out_result);

/*
 Runs the steps of `source` and checks its invariant after each step.

 # Safety
 `ctx` is NULL or a live context. `source` points to `len` readable bytes.
 `out_verdict` points to writable memory.
 */
weft_result_t weft_check(weft_context_t *ctx, weft_string_t source, weft_verdict_t *out_verdict);

/*
 Releases a string from `weft_render` and resets it to `{NULL, 0}`. NULL
 and a reset string are no-ops.

 # Safety
 `result` is NULL or a string from `weft_render` that is not released.
 */
weft_result_t weft_owned_string_free(weft_context_t *ctx, weft_owned_string_t *result);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* WEFT_H */
