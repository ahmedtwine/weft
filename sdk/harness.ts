import { plugin } from "bun";
import { test } from "bun:test";
import inspector from "node:inspector";
import { dirname, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { done, join, type Name, type Report, type State, type Turn, turn, type Use } from "./weft";

type Schema = {
  _zod: { def: { shape?: Record<string, Schema>; options?: Schema[] } };
  parse: (value: unknown) => unknown;
  safeParse: (value: unknown) => { success: boolean; data?: unknown };
};
type Read = [string[], string];
type Used = {
  role: Use["role"];
  run: (input?: unknown) => unknown;
  input?: Schema;
  error?: Schema;
  reads: Read[];
  inputs: { tag?: string; schema: Schema }[];
};
type Failed = [] | [Schema, string];
type Served = { file: string; counts: Int32Array; marks: [number, number][]; starts: number[] };
type Range = { startOffset: number; endOffset: number; count: number };
type Taken = { result: { url: string; functions: { ranges: Range[] }[] }[] };
type Code = Map<string, Set<number>>;

const ADAPTER = "bun";
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
  if (!first) throw new Error(`the diagram has no \`impl ${ADAPTER}\` with a \`module\``);
  const { z } = await import(Bun.resolveSync("zod", dirname(Bun.resolveSync(placed(first), process.cwd()))));
  const tag = (schema: Schema): string | undefined => z.globalRegistry.get(schema)?.weft;
  const reads = (schema: Schema, at: string[]): Read[] => [
    ...[tag(schema)].flatMap((name) => (name ? [[at, name] as Read] : [])),
    ...Object.entries(schema._zod.def.shape ?? {}).flatMap(([key, each]) =>
      reads(each, [...at, key])
    ),
  ];
  const tagged = (schema: Schema) =>
    (schema._zod.def.options ?? [schema]).flatMap((option) => {
      const verb = tag(option);
      return verb ? [{ tag: verb, schema: option }] : [];
    });
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
    const schema = (name: Name) => {
      const found = exported(name) as Schema;
      if (!found?._zod) throw new Error(`\`${name.join(".")}\` is no Zod schema`);
      return found;
    };
    for (const line of lines) {
      const run = exported(line.function);
      if (typeof run !== "function") throw new Error(`\`${line.function.join(".")}\` is no function`);
      const whole = line.schema ? schema(line.schema) : undefined;
      const branches = whole && line.tag ? tagged(whole).filter(({ tag }) => tag === line.tag) : [];
      if (line.tag && branches.length !== 1)
        throw new Error(
          `\`${line.schema?.join(".")}\` has ${branches.length} branches with the tag \`${line.tag}\``
        );
      const input = line.tag ? branches[0].schema : whole;
      const error = line.error ? schema(line.error) : undefined;
      const read = line.role === "read" ? input : error;
      uses.push({
        role: line.role,
        run: run as Used["run"],
        input,
        error,
        reads: read ? reads(read, []) : [],
        inputs: input && line.role !== "read" ? (line.role === "run" ? tagged(input) : [{ schema: input }]) : [],
      });
    }
  }
  const hello = {
    uses: uses.map(
      (use) =>
        use && {
          reads: use.reads.map(([, name]) => name),
          inputs: use.inputs.map(({ tag, schema }) => ({
            tag,
            schema: z.toJSONSchema(schema, { io: "input", unrepresentable: () => ({ not: {} }) }),
          })),
        }
    ),
  };
  return { uses: uses.flatMap((use) => (use ? [use] : [])), hello };
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
  const state = async (failed?: Schema, message = ""): Promise<State> => {
    const out: State = {};
    for (const { role, run, input, error, reads } of uses) {
      const value =
        role === "read" ? input?.parse(await run()) : error?.parse(error === failed ? message : "");
      for (const [at, name] of reads) {
        const [object, key] = name.split(".");
        const found = at.reduce<unknown>((node, part) => (node as State)[part], value);
        out[object] = { ...out[object], [key]: found as string | number };
      }
    }
    return out;
  };
  const called = async (use: Used, input: unknown): Promise<Report | Failed> => {
    const thrown = await Promise.resolve()
      .then(() => use.run(input))
      .then(
        () => [],
        (error: unknown) => [error]
      );
    if (thrown.length === 0) return [];
    const [error] = thrown;
    if (!(error instanceof Error)) return { thrown: String(error) };
    return FAULTS.some((fault) => error instanceof fault) || !use.error
      ? { thrown: `${error.name}: ${error.message}${thrower(error)}` }
      : [use.error, error.message];
  };
  const stepped = async (known: Code, calls: [Used, unknown][]): Promise<Report> => {
    const inputs = calls.map(([use, raw]) => (use.input ? accepted(use.input, raw) : [undefined]));
    if (inputs.some((input) => input.length === 0)) return { refused: true };
    return measured(known, async () => {
      let failed: Failed = [];
      for (const [at, [use]] of calls.entries()) {
        const result = await called(use, inputs[at][0]);
        if (!Array.isArray(result)) return result;
        if (result.length > 0) failed = result;
      }
      return { state: await state(...failed) };
    });
  };
  const starts = uses.filter(({ role }) => role === "start");
  const actions = new Map(
    uses.flatMap((use) =>
      use.inputs.flatMap(({ tag, schema }) => (tag ? [[tag, { ...use, input: schema } as Used] as const] : []))
    )
  );
  const play = async ({ start: first, run: verb = "", input }: Turn): Promise<Report> => {
    if (first !== undefined)
      return stepped(
        ran.start,
        starts.map((use, at) => [use, first[at]])
      );
    const action = actions.get(verb);
    if (!action) return { state: await state() };
    const known = ran.actions.get(verb) ?? new Map();
    ran.actions.set(verb, known);
    return stepped(known, [[action, input]]);
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
