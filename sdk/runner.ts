import { plugin } from "bun";
import inspector from "node:inspector";
import { relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { type Call, done, join, type Report, setup, type Turn, turn } from "./weft";

type Run = (input?: unknown) => unknown;
type Called = { result: unknown } | { thrown: string };
type Served = { file: string; counts: Int32Array; marks: [number, number][]; starts: number[] };
type Range = { startOffset: number; endOffset: number; count: number };
type Taken = { result: { url: string; functions: { ranges: Range[] }[] }[] };
type Code = Map<string, Set<number>>;

const { adapter: ADAPTER, diagram } = setup();
const START = "start";
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
  const calls = new Map<string, Run>();
  const reads: [string, Run][] = [];
  let serves: string | undefined;
  for (const { adapter, modules } of code) {
    if (adapter !== ADAPTER) continue;
    for (const [object, m] of Object.entries(modules)) {
      const exports: Record<string, unknown> = await import(placed(m.path));
      const found = (id: string) => {
        const run = exports[id];
        if (typeof run !== "function")
          throw new Error(`\`module "${object}"\` maps \`${id}\`, and \`${m.path}\` exports no function \`${id}\``);
        return run as Run;
      };
      for (const id of [m.reset, ...Object.values(m.actions)]) if (id) calls.set(id, found(id));
      if (m.state) reads.push([object, found(m.state)]);
      if (m.url) serves = String(await found(m.url)());
    }
  }
  return { calls, reads, hello: { adapter: ADAPTER, serves } };
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
  const { calls, reads, hello } = await loaded();
  const thrown = (error: unknown) =>
    error instanceof Error ? `${error.name}: ${error.message}${thrower(error)}` : String(error);
  const called = async (run: Run, input: unknown): Promise<Called> =>
    Promise.resolve()
      .then(() => run(input))
      .then(
        (result) => ({ result }),
        (error: unknown) => ({ thrown: thrown(error) })
      );
  const reported = async (given: Call[]): Promise<Report> => {
    let result: unknown = null;
    for (const call of given) {
      const run = calls.get(call.function);
      if (!run) return { thrown: `the runner has no function \`${call.function}\`` };
      const ended = await called(run, call.input);
      if (!("result" in ended)) return ended;
      result = ended.result ?? null;
    }
    try {
      const states: Record<string, unknown> = {};
      for (const [object, read] of reads) states[object] = JSON.parse(JSON.stringify((await read()) ?? null));
      return { states, result: JSON.parse(JSON.stringify(result)) };
    } catch (error: unknown) {
      return { thrown: thrown(error) };
    }
  };
  const play = ({ step, calls: given }: Turn): Promise<Report> => {
    if (step === undefined) return reported(given);
    if (step === START) return measured(ran.start, () => reported(given));
    const known = ran.actions.get(step) ?? new Map();
    ran.actions.set(step, known);
    return measured(known, () => reported(given));
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
