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
typedef int (*weft_add_t)(void *, weft_string_t, weft_string_t);
typedef int (*weft_values_t)(void *, weft_string_t, _Bool, weft_string_t *);
typedef int (*weft_score_t)(void *, double, weft_string_t);

static void *weft_ctx;
static weft_error_t weft_last;
static weft_free_t weft_release;
static weft_ask_t weft_asks[3];
static weft_done_t weft_end;
static weft_add_t weft_add;
static weft_values_t weft_values;
static weft_score_t weft_score;

static const char *weft_load(const char *path) {
	void *lib = dlopen(path, RTLD_NOW);
	if (!lib) return dlerror();
	weft_new_t make = (weft_new_t)dlsym(lib, "weft_context_new");
	weft_last = (weft_error_t)dlsym(lib, "weft_context_last_error");
	weft_release = (weft_free_t)dlsym(lib, "weft_owned_string_free");
	weft_asks[0] = (weft_ask_t)dlsym(lib, "weft_diagram_join");
	weft_asks[1] = (weft_ask_t)dlsym(lib, "weft_diagram_turn");
	weft_asks[2] = (weft_ask_t)dlsym(lib, "weft_generate");
	weft_end = (weft_done_t)dlsym(lib, "weft_diagram_done");
	weft_add = (weft_add_t)dlsym(lib, "weft_pool_add");
	weft_values = (weft_values_t)dlsym(lib, "weft_pool_generate");
	weft_score = (weft_score_t)dlsym(lib, "weft_target");
	if (!make || !weft_last || !weft_release || !weft_asks[0] || !weft_asks[1] || !weft_asks[2] || !weft_end || !weft_add || !weft_values || !weft_score) return "libweft lacks a weft_* call";
	weft_ctx = make();
	return 0;
}

static int weft_ask(int call, const char *data, size_t len, weft_string_t *out) {
	weft_string_t in = {data, len};
	return weft_asks[call](weft_ctx, in, out);
}

static int weft_pool_add(const char *pool, size_t named, const char *value, size_t len) {
	weft_string_t name = {pool, named}, in = {value, len};
	return weft_add(weft_ctx, name, in);
}

static int weft_pool_generate(const char *pool, size_t named, _Bool consume, weft_string_t *out) {
	weft_string_t name = {pool, named};
	return weft_values(weft_ctx, name, consume, out);
}

static int weft_target(double value, const char *label, size_t len) {
	weft_string_t in = {label, len};
	return weft_score(weft_ctx, value, in);
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

const (
	join = iota
	turns
	generate
)

const (
	stopTest = -5
	assume   = -6
)

type stopped string

func (s stopped) Error() string { return string(s) }

func checked(code C.int) error {
	switch code {
	case stopTest:
		return stopped("stop_test")
	case assume:
		return stopped("assume")
	}
	if code != 0 {
		said := C.weft_error()
		return errors.New(C.GoStringN(said.data, C.int(said.len)))
	}
	return nil
}

func given(out *C.weft_string_t, code C.int) ([]byte, error) {
	if err := checked(code); err != nil {
		return nil, err
	}
	said := C.GoBytes(unsafe.Pointer(out.data), C.int(out.len))
	C.weft_free(out)
	return said, nil
}

func ask(call int, message []byte) ([]byte, error) {
	data := C.CBytes(message)
	defer C.free(data)
	var out C.weft_string_t
	return given(&out, C.weft_ask(C.int(call), (*C.char)(data), C.size_t(len(message)), &out))
}

func done(message []byte) error {
	data := C.CBytes(message)
	defer C.free(data)
	return checked(C.weft_said_done((*C.char)(data), C.size_t(len(message))))
}

func poolAdd(pool string, value []byte) error {
	name, data := C.CString(pool), C.CBytes(value)
	defer C.free(unsafe.Pointer(name))
	defer C.free(data)
	return checked(C.weft_pool_add(name, C.size_t(len(pool)), (*C.char)(data), C.size_t(len(value))))
}

func poolGenerate(pool string, consume bool) ([]byte, error) {
	name := C.CString(pool)
	defer C.free(unsafe.Pointer(name))
	var out C.weft_string_t
	return given(&out, C.weft_pool_generate(name, C.size_t(len(pool)), C.bool(consume), &out))
}

func target(value float64, label string) error {
	text := C.CString(label)
	defer C.free(unsafe.Pointer(text))
	return checked(C.weft_target(C.double(value), text, C.size_t(len(label))))
}
