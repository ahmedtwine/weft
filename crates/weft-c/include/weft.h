#pragma once

#include <stddef.h>
#include <stdint.h>

typedef enum {
  WEFT_OK = 0,
  WEFT_E_PARSE = -1,
  WEFT_E_INVALID_ARG = -2,
  WEFT_E_RENDER = -3,
  WEFT_E_REFUSED = -4,
} weft_result_t;

typedef enum {
  WEFT_FORMAT_SVG_LIGHT = 0,
  WEFT_FORMAT_SVG_DARK = 1,
  WEFT_FORMAT_ASCII = 2,
} weft_format_t;

typedef enum {
  WEFT_STATUS_NO_CHECK = 0,
  WEFT_STATUS_HOLDS = 1,
  WEFT_STATUS_FAILS = 2,
} weft_status_t;

typedef struct weft_context_t weft_context_t;

typedef struct {
  const char *data;
  size_t len;
} weft_string_t;

typedef weft_string_t weft_owned_string_t;

typedef struct {
  uint32_t status;
  size_t step;
} weft_verdict_t;

weft_context_t *weft_context_new(void);

weft_result_t weft_context_free(weft_context_t *ctx);

weft_string_t weft_context_last_error(const weft_context_t *ctx);

weft_result_t weft_render(weft_context_t *ctx,
                          weft_string_t config,
                          weft_string_t source,
                          uint32_t format,
                          weft_owned_string_t *out_result);

weft_result_t weft_check(weft_context_t *ctx,
                         weft_string_t config,
                         weft_string_t source,
                         weft_verdict_t *out_verdict);

weft_result_t weft_owned_string_free(weft_context_t *ctx, weft_owned_string_t *result);

weft_result_t weft_diagram_join(weft_context_t *ctx,
                                weft_string_t name,
                                weft_owned_string_t *out_code);

weft_result_t weft_diagram_turn(weft_context_t *ctx,
                                weft_string_t report,
                                weft_owned_string_t *out_turn);

weft_result_t weft_diagram_done(weft_context_t *ctx, weft_string_t said);
