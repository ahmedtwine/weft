import { plugin } from "bun";
import { test } from "bun:test";
import inspector from "node:inspector";
import { dirname, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { type Call, done, join, type Name, type Report, type Turn, turn, type Use } from "./weft";

type Schema = {
  _zod: unknown;
  parse: (value: unknown) => unknown;
  safeParse: (value: unknown) => { success: boolean; data?: unknown };
};
type Run = (input?: unknown) => unknown;
type Used = {
  role: Use["role"];
  run?: Run;
  method: string;
  input?: Schema;
  output?: Schema;
  error?: Schema;
};
type Failed = [] | [Schema, string];
type Called = { result: unknown } | { failed: [Schema, string] } | { thrown: string };
type Served = { file: string; counts: Int32Array; marks: [number, number][]; starts: number[] };
type Range = { startOffset: number; endOffset: number; count: number };
type Taken = { result: { url: string; functions: { ranges: Range[] }[] }[] };
type Code = Map<string, Set<number>>;

const ADAPTER = "bun";
const START = "start";
const SAMPLES = 8;
const FAULTS = [TypeError, RangeError, ReferenceError, SyntaxError, EvalError, URIError];
const SOURCE = new RegExp(
  `^${process.cwd().replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}/(?!.*node_modules/).*\\.[cm]?[jt]sx?$`
);
const BASE64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

const at = (name: string) => {
  const value = Bun.env[name];
  if (!value) throw new Error(`${name} is not set`);
  return value;
};
const served = new Map<string, Served>();

const marks = (mappings: string, starts: number[]) => {
  const out: [number, number][] = [];
  let line = 0;
  mappings.split(";").forEach((row, generated) => {
    let column = 0;
    for (const segment of row.split(",").filter(Boolean)) {
      const fields: number[] = [];
      let value = 0;
      let shift = 0;
      for (const char of segment) {
        const digit = BASE64.indexOf(char);
        value += (digit & 31) << shift;
        shift += 5;
        if (digit < 32) {
          fields.push(value & 1 ? -(value >> 1) : value >> 1);
          value = 0;
          shift = 0;
        }
      }
      column += fields[0];
      if (fields.length > 2) {
        line += fields[2];
        out.push([starts[generated] + column, line + 1]);
      }
    }
  });
  return out;
};

plugin({
  name: "weft",
  setup(build) {
    build.onLoad({ filter: SOURCE }, async ({ path }) => {
      const file = relative(process.cwd(), path);
      const built = await Bun.build({
        entrypoints: [path],
        external: ["*"],
        target: "bun",
        sourcemap: "external",
      });
      const [code, map] = ["entry-point", "sourcemap"].map((kind) =>
        built.outputs.find((output) => output.kind === kind)
      );
      if (!code || !map) throw new Error(`bun build gives no code for ${file}`);
      const contents = await code.text();
      const starts = contents
        .split("\n")
        .reduce((all, text) => [...all, all[all.length - 1] + text.length + 1], [0]);
      served.set(path, {
        file,
        counts: new Int32Array(contents.length),
        marks: marks((await map.json()).mappings, starts),
        starts,
      });
      return { contents, loader: "js" };
    });
  },
});

const session = new inspector.Session();
session.connect();
const post = <T>(method: string, params = {}) =>
  new Promise<T>((resolve, reject) =>
    session.post(method, params, (error: Error | null, result: unknown) =>
      error ? reject(error) : resolve(result as T)
    )
  );
await post("Profiler.enable");
await post("Profiler.startPreciseCoverage", { callCount: true, detailed: true });

const taken = async (known: Code) => {
  const { result } = await post<Taken>("Profiler.takePreciseCoverage");
  for (const script of result) {
    const each = script.url.startsWith("file://")
      ? served.get(fileURLToPath(script.url))
      : undefined;
    if (!each || !script.functions.some(({ ranges }) => ranges[0].count > 0)) continue;
    const wholes = script.functions.map(({ ranges: [whole] }) => whole);
    const widest = Math.max(...wholes.map(({ endOffset }) => endOffset));
    if (
      widest > each.counts.length ||
      (wholes.some(({ startOffset }) => startOffset === 0) && widest !== each.counts.length)
    )
      throw new Error(`bun runs other code than weft builds for ${each.file}`);
    each.counts.fill(0);
    for (const { ranges } of script.functions.sort(
      (a, b) =>
        a.ranges[0].startOffset - b.ranges[0].startOffset ||
        b.ranges[0].endOffset - a.ranges[0].endOffset
    ))
      for (const range of ranges) each.counts.fill(range.count, range.startOffset, range.endOffset);
    const lines = known.get(each.file) ?? new Set<number>();
    known.set(each.file, lines);
    for (const [offset, line] of each.marks) if (each.counts[offset] > 0) lines.add(line);
  }
};

const sampled = new Map<Code, number>();
const measured = async <T>(known: Code, run: () => T | Promise<T>) => {
  const samples = sampled.get(known) ?? 0;
  if (samples >= SAMPLES) return run();
  sampled.set(known, samples + 1);
  await post("Profiler.takePreciseCoverage");
  try {
    return await run();
  } finally {
    await taken(known);
  }
};

const spans = (known: Code) => {
  const out: { file: string; from: number; to: number }[] = [];
  for (const each of served.values()) {
    const ran = known.get(each.file);
    if (!ran) continue;
    let open: { file: string; from: number; to: number } | undefined;
    for (const line of [...new Set(each.marks.map(([, line]) => line))].sort((a, b) => a - b)) {
      if (!ran.has(line)) open = undefined;
      else if (open) open.to = line;
      else out.push((open = { file: each.file, from: line, to: line }));
    }
  }
  return out;
};

const code = join(at("WEFT_DIAGRAM"));

const placed = (place: string) => (place.startsWith(".") ? resolve(place) : place);

const loaded = async () => {
  const [first] = code.flatMap(({ adapter, modules }) =>
    adapter === ADAPTER ? Object.values(modules) : []
  );
  if (!first) throw new Error(`the diagram has no \`impl "${ADAPTER}"\` with a \`module\``);
  const { z } = await import(Bun.resolveSync("zod", dirname(Bun.resolveSync(placed(first), process.cwd()))));
  const json = (schema: Schema | undefined, io: "input" | "output") =>
    schema && z.toJSONSchema(schema, { io, unrepresentable: () => ({ not: {} }) });
  const uses: (Used | null)[] = [];
  for (const { adapter, modules: places, uses: lines } of code) {
    if (adapter !== ADAPTER) {
      uses.push(...lines.map(() => null));
      continue;
    }
    const modules: Record<string, Record<string, unknown>> = Object.fromEntries(
      await Promise.all(
        Object.entries(places).map(async ([module, place]) => [
          module,
          await import(placed(place)),
        ])
      )
    );
    const exported = ([module, name]: Name) => {
      if (!(name in modules[module])) throw new Error(`\`${module}\` exports no \`${name}\``);
      return modules[module][name];
    };
    const schema = (name: Name | null) => {
      if (!name) return undefined;
      const found = exported(name) as Schema;
      if (!found?._zod) throw new Error(`\`${name.join(".")}\` is no Zod schema`);
      return found;
    };
    for (const line of lines) {
      const run = line.from ? undefined : exported(line.function);
      if (!line.from && typeof run !== "function")
        throw new Error(`\`${line.function.join(".")}\` is no function`);
      uses.push({
        role: line.role,
        run: run as Run | undefined,
        method: line.function[1],
        input: schema(line.input),
        output: schema(line.output),
        error: schema(line.error),
      });
    }
  }
  const hello = {
    uses: uses.map(
      (use) =>
        use && {
          input: json(use.input, "input"),
          output: json(use.output, "output"),
          error: json(use.error, "output"),
        }
    ),
  };
  return { uses, hello };
};

const accepted = (schema: Schema, value: unknown): [unknown] | [] => {
  try {
    const result = schema.safeParse(value);
    return result.success ? [result.data] : [];
  } catch {
    return [];
  }
};

const thrower = (error: Error) => {
  for (const frame of error.stack?.split("\n") ?? []) {
    const [, path, row, column] = frame.match(/\(?([^()\s]+):(\d+):(\d+)\)?$/) ?? [];
    const each = served.get(path);
    if (!each) continue;
    const offset = each.starts[Number(row) - 1] + Number(column) - 1;
    const [, line] = each.marks.findLast(([mark]) => mark <= offset) ?? [];
    if (line) return ` at ${each.file}:${line}`;
  }
  return "";
};

const ran = { start: new Map() as Code, actions: new Map<string, Code>() };

const played = async () => {
  const { uses, hello } = await loaded();
  const pool = new Map<number, unknown>();
  const target = (use: Used, from: number | null): Run => {
    if (use.run) return use.run;
    const value = pool.get(from ?? -1) as Run & Record<string, Run>;
    return use.method ? value[use.method].bind(value) : value;
  };
  const called = async (use: Used, call: Call, input: unknown): Promise<Called> => {
    const [result, ...thrown] = await Promise.resolve()
      .then(() => target(use, call.from)(input))
      .then(
        (result) => [result],
        (error: unknown) => [undefined, error]
      );
    if (thrown.length === 0) return { result };
    const [error] = thrown;
    if (!(error instanceof Error)) return { thrown: String(error) };
    return FAULTS.some((fault) => error instanceof fault) || !use.error
      ? { thrown: `${error.name}: ${error.message}${thrower(error)}` }
      : { failed: [use.error, error.message] };
  };
  const reported = async (calls: Call[], inputs: unknown[][]): Promise<Report> => {
    const outputs: Record<number, unknown> = {};
    const errors: Record<number, unknown> = {};
    const added: number[] = [];
    let [failed, message]: Failed = [];
    for (const [at, call] of calls.entries()) {
      const use = uses[call.use];
      if (!use) throw new Error(`the diagram has no line ${call.use} for \`impl "${ADAPTER}"\``);
      const [input] = inputs[at];
      const ended = await called(use, call, input);
      if ("thrown" in ended) return ended;
      if ("failed" in ended) {
        [failed, message] = ended.failed;
        continue;
      }
      if (call.add !== null) {
        pool.set(call.add, ended.result);
        added.push(call.add);
      }
      if (use.output) outputs[call.use] = use.output.parse({ input, result: ended.result });
    }
    for (const [at, use] of uses.entries()) {
      if (use?.role === "read") outputs[at] = use.output?.parse(await use.run?.());
      if (use?.error) errors[at] = use.error.parse(use.error === failed ? message : "");
    }
    return { outputs, errors, added };
  };
  const stepped = async (calls: Call[], known?: Code): Promise<Report> => {
    const inputs = calls.map(({ use, input }) => {
      const schema = uses[use]?.input;
      return schema ? accepted(schema, input) : [undefined];
    });
    if (inputs.some((input) => input.length === 0)) return { refused: true };
    return known ? measured(known, () => reported(calls, inputs)) : reported(calls, inputs);
  };
  const play = ({ step, calls }: Turn): Promise<Report> => {
    if (step === undefined) return stepped(calls);
    if (step === START) {
      pool.clear();
      return stepped(calls, ran.start);
    }
    const known = ran.actions.get(step) ?? new Map();
    ran.actions.set(step, known);
    return stepped(calls, known);
  };
  let now = turn(hello);
  while (!now.done) now = turn(await play(now));
};

test("weft", async () => {
  done(
    await played().then(
      () => ({
        ran: {
          start: spans(ran.start),
          actions: Object.fromEntries([...ran.actions].map(([verb, known]) => [verb, spans(known)])),
        },
      }),
      (error: unknown) => ({
        error: error instanceof Error ? (error.stack ?? error.message) : String(error),
      })
    )
  );
});
