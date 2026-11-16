import { plugin } from "bun";
import inspector from "node:inspector";
import { dirname, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { schemas, states } from "./state";
import { TestCase } from "./testcase";
import { AssumeFailed, type Call, done, join, type Report, setup, StopTest, type Turn, turn } from "./weft";

type Schema = {
  _zod: unknown;
  parse: (value: unknown) => unknown;
  safeParse: (value: unknown) => { success: boolean; data?: unknown };
};
type Run = (input: unknown, tc: TestCase) => unknown;
type Used = { run: Run; input?: Schema };
type Called = { result: unknown } | { thrown: string } | { refused: string };
type Served = { file: string; counts: Int32Array; marks: [number, number][]; starts: number[] };
type Range = { startOffset: number; endOffset: number; count: number };
type Taken = { result: { url: string; functions: { ranges: Range[] }[] }[] };
type Code = Map<string, Set<number>>;

const { adapter: ADAPTER, diagram } = setup();
const START = "start";
const RESET = "reset";
const URL = "url";
const SAMPLES = 8;
const SOURCE = new RegExp(
  `^${process.cwd().replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}/(?!.*node_modules/).*\\.[cm]?[jt]sx?$`
);
const BASE64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

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

const code = join(diagram);

const placed = (place: string) => (place.startsWith(".") ? resolve(place) : place);

const loaded = async () => {
  const [first] = code.flatMap(({ adapter, modules }) =>
    adapter === ADAPTER ? Object.values(modules) : []
  );
  if (!first) throw new Error(`the diagram has no \`impl "${ADAPTER}"\` with a \`module\``);
  const { z } = await import(Bun.resolveSync("zod", dirname(Bun.resolveSync(placed(first), process.cwd()))));
  const json = (schema: Schema | undefined, io: "input" | "output") =>
    schema && z.toJSONSchema(schema, { io, unrepresentable: () => ({ not: {} }) });
  const start: Used[] = [];
  const actions: Used[] = [];
  const said = { start: [] as unknown[], actions: [] as unknown[] };
  let serves: string | undefined;
  for (const { adapter, modules: places } of code) {
    if (adapter !== ADAPTER) continue;
    for (const [module, place] of Object.entries(places)) {
      const exports: Record<string, unknown> = await import(placed(place));
      for (const [name, run] of Object.entries(exports)) {
        if (typeof run !== "function") continue;
        const input = exports[`${name}Schema`] as Schema | undefined;
        const each = { function: [module, name], input: json(input, "input") ?? null };
        if (name === URL) serves = String(await (run as () => unknown)());
        else if (name === RESET) {
          start.push({ run: run as Run, input });
          said.start.push(each);
        } else {
          actions.push({ run: run as Run, input });
          said.actions.push(each);
        }
      }
    }
  }
  const hello = {
    adapter: ADAPTER,
    serves,
    states: schemas((type) => json(type as Schema, "output")),
    ...said,
  };
  return { start, actions, hello, tc: new TestCase((schema) => json(schema as Schema, "input")) };
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
  const { start, actions, hello, tc } = await loaded();
  const called = async (use: Used, input: unknown): Promise<Called> => {
    const [result, ...thrown] = await Promise.resolve()
      .then(() => use.run(input, tc))
      .then(
        (result) => [result],
        (error: unknown) => [undefined, error]
      );
    if (thrown.length === 0) return { result };
    const [error] = thrown;
    if (error instanceof StopTest) return { refused: "stop_test" };
    if (error instanceof AssumeFailed) return { refused: "assume" };
    if (!(error instanceof Error)) return { thrown: String(error) };
    return { thrown: `${error.name}: ${error.message}${thrower(error)}` };
  };
  let uses = actions;
  const reported = async (calls: Call[], inputs: unknown[][]): Promise<Report> => {
    for (const [at, call] of calls.entries()) {
      const use = uses[call.use];
      if (!use) throw new Error(`the runner has no function ${call.use}`);
      const [input] = inputs[at];
      const ended = await called(use, input);
      if (!("result" in ended)) return ended;
    }
    return { states: {} };
  };
  const stepped = async (calls: Call[], known?: Code): Promise<Report> => {
    const inputs = calls.map(({ use, input }) => {
      const schema = uses[use]?.input;
      return schema ? accepted(schema, input) : [undefined];
    });
    if (inputs.some((input) => input.length === 0)) return { refused: "schema" };
    const report = await (known
      ? measured(known, () => reported(calls, inputs))
      : reported(calls, inputs));
    if (!("states" in report)) return report;
    try {
      return { ...report, states: states() };
    } catch (error: unknown) {
      return { thrown: error instanceof Error ? `${error.name}: ${error.message}${thrower(error)}` : String(error) };
    }
  };
  const play = ({ step, calls }: Turn): Promise<Report> => {
    uses = step === START ? start : actions;
    if (step === undefined) return stepped(calls);
    if (step === START) return stepped(calls, ran.start);
    const known = ran.actions.get(step) ?? new Map();
    ran.actions.set(step, known);
    return stepped(calls, known);
  };
  let now = turn(hello);
  while (!now.done) now = turn(await play(now));
};

export const run = async () =>
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
