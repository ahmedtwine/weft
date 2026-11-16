import koffi from "koffi";

type Owned = { data: unknown; len: number };
export type Module = {
  path: string;
  reset: string | null;
  state: string | null;
  url: string | null;
  actions: Record<string, string>;
};
export type Code = { adapter: string; modules: Record<string, Module> }[];
export type Call = { function: string; input: unknown };
export type Turn = { step?: string; calls: Call[]; done?: true };
export type Report = { states: Record<string, unknown>; result: unknown } | { thrown: string };

const FAILED = -7;

export const setup = (): { diagram: string; lib: string; adapter: string } => {
  const text = process.env.WEFT;
  if (!text) throw new Error("WEFT is not set");
  return JSON.parse(text);
};

const opened = (library = setup().lib) => {
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
    start: lib.func(
      "int weft_test_start(weft_context_t *ctx, weft_string_t adapter, weft_string_t file, _Out_ weft_owned_string_t *out_joined)"
    ),
    end: lib.func("int weft_test_end(weft_context_t *ctx, _Out_ weft_owned_string_t *out_report)"),
  };
};

type Weft = ReturnType<typeof opened>;
type Asked = "join" | "turn" | "start";
let weft: Weft | undefined;
const call = () => (weft ??= opened());

const given = (text: string) => ({ data: text, len: Buffer.byteLength(text) });
const json = (value: unknown) => given(JSON.stringify(value));
const checked = (code: number) => {
  if (code !== 0) throw new Error(call().error(call().ctx).data);
};
const taken = (out: Owned) => {
  const text = Buffer.from(koffi.decode(out.data, "uint8_t", out.len)).toString();
  call().free(call().ctx, out);
  return text;
};
const asked = (ask: Asked, ...args: unknown[]) => {
  const out: Owned = { data: null, len: 0 };
  checked(call()[ask](call().ctx, ...args, out));
  return JSON.parse(taken(out));
};

export const join = (name: string): Code => asked("join", given(name));
export const turn = (report: unknown): Turn => asked("turn", json(report));
export const done = (said: unknown) => checked(call().done(call().ctx, json(said)));
export const started = (library: string, adapter: string, file: string) => {
  weft ??= opened(library);
  return asked("start", given(adapter), given(file)) as { cwd: string; env: Record<string, string> };
};
export const ended = () => {
  const out: Owned = { data: null, len: 0 };
  const code = call().end(call().ctx, out);
  if (code !== 0 && code !== FAILED) throw new Error(call().error(call().ctx).data);
  return { failed: code === FAILED, report: taken(out) };
};
export const setenv = (env: Record<string, string>) => {
  const set = koffi.load(null).func("int setenv(const char *name, const char *value, int overwrite)");
  for (const [name, value] of Object.entries(env)) {
    process.env[name] = value;
    set(name, value, 1);
  }
};
