import koffi from "koffi";

type Pairs = Record<string, string | number>;
export type State = Record<string, Pairs>;
export type Turn = {
  reset?: true;
  run?: string;
  object?: string;
  read?: true;
  done?: true;
  failed?: string[];
};
export type Report = { state?: State; input?: string; thrown?: string; refused?: true };
type Owned = { data: unknown; len: number };

const library = process.env.WEFT_LIB;
if (!library) throw new Error("WEFT_LIB is not set");
process.env.HEGEL_LIBHEGEL_PATH = library;

const lib = koffi.load(library);
koffi.struct("weft_string_t", { data: "const char *", len: "size_t" });
koffi.struct("weft_owned_string_t", { data: "void *", len: "size_t" });
koffi.opaque("weft_context_t");
const call = {
  context: lib.func("weft_context_t *weft_context_new(void)"),
  error: lib.func("weft_string_t weft_context_last_error(const weft_context_t *ctx)"),
  free: lib.func("int weft_owned_string_free(weft_context_t *ctx, _Inout_ weft_owned_string_t *result)"),
  join: lib.func("int weft_diagram_join(weft_context_t *ctx, weft_string_t name, weft_string_t hello)"),
  turn: lib.func(
    "int weft_diagram_turn(weft_context_t *ctx, weft_string_t report, _Out_ weft_owned_string_t *out_turn)"
  ),
  done: lib.func("int weft_diagram_done(weft_context_t *ctx, weft_string_t said)"),
};

const ctx = call.context();
const given = (value: unknown) => {
  const text = typeof value === "string" ? value : JSON.stringify(value);
  return { data: text, len: Buffer.byteLength(text) };
};
const checked = (code: number) => {
  if (code !== 0) throw new Error(call.error(ctx).data);
};

export const join = (name: string, hello: unknown) => checked(call.join(ctx, given(name), given(hello)));
export const done = (said: unknown) => checked(call.done(ctx, given(said)));
export const turn = (report: Report): Turn => {
  const out: Owned = { data: null, len: 0 };
  checked(call.turn(ctx, given(report), out));
  const next = JSON.parse(Buffer.from(koffi.decode(out.data, "uint8_t", out.len)).toString());
  call.free(ctx, out);
  return next;
};
