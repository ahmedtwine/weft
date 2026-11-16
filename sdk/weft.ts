import koffi from "koffi";

type Owned = { data: unknown; len: number };
export type Code = { adapter: string; modules: Record<string, string> }[];
export type Call = { use: number; input: unknown };
export type Turn = { step?: string; calls: Call[]; done?: true };
export type Report =
  | { states: Record<string, unknown> }
  | { thrown: string }
  | { refused: string };

export class StopTest extends Error {}
export class AssumeFailed extends Error {}

const STOP_TEST = -5;
const ASSUME = -6;

const opened = () => {
  const library = process.env.WEFT_LIB;
  if (!library) throw new Error("WEFT_LIB is not set");
  const lib = koffi.load(library);
  koffi.struct("weft_string_t", { data: "const char *", len: "size_t" });
  koffi.struct("weft_owned_string_t", { data: "void *", len: "size_t" });
  koffi.opaque("weft_context_t");
  return {
    ctx: lib.func("weft_context_t *weft_context_new(void)")(),
    error: lib.func("weft_string_t weft_context_last_error(const weft_context_t *ctx)"),
    free: lib.func("int weft_owned_string_free(weft_context_t *ctx, _Inout_ weft_owned_string_t *result)"),
    join: lib.func(
      "int weft_diagram_join(weft_context_t *ctx, weft_string_t name, _Out_ weft_owned_string_t *out_code)"
    ),
    turn: lib.func(
      "int weft_diagram_turn(weft_context_t *ctx, weft_string_t report, _Out_ weft_owned_string_t *out_turn)"
    ),
    done: lib.func("int weft_diagram_done(weft_context_t *ctx, weft_string_t said)"),
    generate: lib.func(
      "int weft_generate(weft_context_t *ctx, weft_string_t schema, _Out_ weft_owned_string_t *out_value)"
    ),
    poolAdd: lib.func("int weft_pool_add(weft_context_t *ctx, weft_string_t pool, weft_string_t value)"),
    poolGenerate: lib.func(
      "int weft_pool_generate(weft_context_t *ctx, weft_string_t pool, bool consume, _Out_ weft_owned_string_t *out_value)"
    ),
    target: lib.func("int weft_target(weft_context_t *ctx, double value, weft_string_t label)"),
  };
};

type Weft = ReturnType<typeof opened>;
type Asked = "join" | "turn" | "generate" | "poolGenerate";
let weft: Weft | undefined;
const call = () => (weft ??= opened());

const given = (text: string) => ({ data: text, len: Buffer.byteLength(text) });
const json = (value: unknown) => given(JSON.stringify(value));
const checked = (code: number) => {
  if (code === STOP_TEST) throw new StopTest();
  if (code === ASSUME) throw new AssumeFailed();
  if (code !== 0) throw new Error(call().error(call().ctx).data);
};
const asked = (ask: Asked, ...args: unknown[]) => {
  const out: Owned = { data: null, len: 0 };
  checked(call()[ask](call().ctx, ...args, out));
  const said = JSON.parse(Buffer.from(koffi.decode(out.data, "uint8_t", out.len)).toString());
  call().free(call().ctx, out);
  return said;
};

export const join = (name: string): Code => asked("join", given(name));
export const turn = (report: unknown): Turn => asked("turn", json(report));
export const done = (said: unknown) => checked(call().done(call().ctx, json(said)));
export const generate = (schema: unknown): unknown => asked("generate", json(schema));
export const poolAdd = (pool: string, value: unknown) =>
  checked(call().poolAdd(call().ctx, given(pool), json(value)));
export const poolGenerate = (pool: string, consume: boolean): unknown =>
  asked("poolGenerate", given(pool), consume);
export const target = (value: number, label: string) =>
  checked(call().target(call().ctx, value, given(label)));
export const setenv = (env: Record<string, string>) => {
  const set = koffi.load(null).func("int setenv(const char *name, const char *value, int overwrite)");
  for (const [name, value] of Object.entries(env)) {
    process.env[name] = value;
    set(name, value, 1);
  }
};
