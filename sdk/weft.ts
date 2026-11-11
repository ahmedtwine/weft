import koffi from "koffi";

type Pairs = Record<string, string | number>;
type Owned = { data: unknown; len: number };
export type State = Record<string, Pairs>;
export type Name = [module: string, name: string];
export type Use = {
  role: "start" | "read" | "run";
  function: Name;
  schema: Name | null;
  tag: string | null;
  error: Name | null;
};
export type Code = { adapter: string; modules: Record<string, string>; uses: Use[] }[];
export type Turn = { start?: unknown[]; run?: string; input?: unknown; done?: true };
export type Report = { state: State } | { thrown: string } | { refused: true };

const library = process.env.WEFT_LIB;
if (!library) throw new Error("WEFT_LIB is not set");

const lib = koffi.load(library);
koffi.struct("weft_string_t", { data: "const char *", len: "size_t" });
koffi.struct("weft_owned_string_t", { data: "void *", len: "size_t" });
koffi.opaque("weft_context_t");
const call = {
  context: lib.func("weft_context_t *weft_context_new(void)"),
  error: lib.func("weft_string_t weft_context_last_error(const weft_context_t *ctx)"),
  free: lib.func("int weft_owned_string_free(weft_context_t *ctx, _Inout_ weft_owned_string_t *result)"),
  join: lib.func(
    "int weft_diagram_join(weft_context_t *ctx, weft_string_t name, _Out_ weft_owned_string_t *out_code)"
  ),
  turn: lib.func(
    "int weft_diagram_turn(weft_context_t *ctx, weft_string_t report, _Out_ weft_owned_string_t *out_turn)"
  ),
  done: lib.func("int weft_diagram_done(weft_context_t *ctx, weft_string_t said)"),
};

const ctx = call.context();
const given = (text: string) => ({ data: text, len: Buffer.byteLength(text) });
const checked = (code: number) => {
  if (code !== 0) throw new Error(call.error(ctx).data);
};
const asked = (ask: typeof call.join, text: string) => {
  const out: Owned = { data: null, len: 0 };
  checked(ask(ctx, given(text), out));
  const said = JSON.parse(Buffer.from(koffi.decode(out.data, "uint8_t", out.len)).toString());
  call.free(ctx, out);
  return said;
};

export const join = (name: string): Code => asked(call.join, name);
export const turn = (report: unknown): Turn => asked(call.turn, JSON.stringify(report));
export const done = (said: unknown) => checked(call.done(ctx, given(JSON.stringify(said))));
