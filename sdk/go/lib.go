package weft

/*
#cgo linux LDFLAGS: -ldl
#include <dlfcn.h>
#include <stddef.h>
#include <stdlib.h>

typedef struct { const char *data; size_t len; } weft_string_t;
typedef void *(*weft_new_t)(void);
typedef weft_string_t (*weft_error_t)(const void *);
typedef int (*weft_free_t)(void *, weft_string_t *);
typedef int (*weft_ask_t)(void *, weft_string_t, weft_string_t *);
typedef int (*weft_done_t)(void *, weft_string_t);

static void *weft_ctx;
static weft_error_t weft_last;
static weft_free_t weft_release;
static weft_ask_t weft_join, weft_turn;
static weft_done_t weft_end;

static const char *weft_load(const char *path) {
	void *lib = dlopen(path, RTLD_NOW);
	if (!lib) return dlerror();
	weft_new_t make = (weft_new_t)dlsym(lib, "weft_context_new");
	weft_last = (weft_error_t)dlsym(lib, "weft_context_last_error");
	weft_release = (weft_free_t)dlsym(lib, "weft_owned_string_free");
	weft_join = (weft_ask_t)dlsym(lib, "weft_diagram_join");
	weft_turn = (weft_ask_t)dlsym(lib, "weft_diagram_turn");
	weft_end = (weft_done_t)dlsym(lib, "weft_diagram_done");
	if (!make || !weft_last || !weft_release || !weft_join || !weft_turn || !weft_end) return "libweft lacks a weft_* call";
	weft_ctx = make();
	return 0;
}

static int weft_ask(int turn, const char *data, size_t len, weft_string_t *out) {
	weft_string_t in = {data, len};
	return (turn ? weft_turn : weft_join)(weft_ctx, in, out);
}

static int weft_said_done(const char *data, size_t len) {
	weft_string_t in = {data, len};
	return weft_end(weft_ctx, in);
}

static weft_string_t weft_error(void) { return weft_last(weft_ctx); }
static void weft_free(weft_string_t *s) { weft_release(weft_ctx, s); }
*/
import "C"

import (
	"errors"
	"unsafe"
)

func load(path string) error {
	text := C.CString(path)
	defer C.free(unsafe.Pointer(text))
	if failed := C.weft_load(text); failed != nil {
		return errors.New(C.GoString(failed))
	}
	return nil
}

func lastError() error {
	said := C.weft_error()
	return errors.New(C.GoStringN(said.data, C.int(said.len)))
}

func ask(turn bool, message []byte) ([]byte, error) {
	data := C.CBytes(message)
	defer C.free(data)
	var out C.weft_string_t
	which := C.int(0)
	if turn {
		which = 1
	}
	if C.weft_ask(which, (*C.char)(data), C.size_t(len(message)), &out) != 0 {
		return nil, lastError()
	}
	said := C.GoBytes(unsafe.Pointer(out.data), C.int(out.len))
	C.weft_free(&out)
	return said, nil
}

func done(message []byte) error {
	data := C.CBytes(message)
	defer C.free(data)
	if C.weft_said_done((*C.char)(data), C.size_t(len(message))) != 0 {
		return lastError()
	}
	return nil
}
