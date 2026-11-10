import { plugin } from "bun";
import { spyOn, test } from "bun:test";
import inspector from "node:inspector";
import { dirname, relative } from "node:path";
import { fileURLToPath } from "node:url";
import * as hegel from "@hegeldev/hegel";
import * as gs from "@hegeldev/hegel/generators";
import { done, join, type Report, type State, type Turn, turn } from "./weft";
import { accepted, generators, type Schema } from "./zod";

type Tagged = Schema & {
  _zod: { def: { shape?: Record<string, Tagged>; options?: Tagged[] } };
};
type Drawable = Schema | gs.Generator<unknown>;
type Action = { input: Tagged; run: (input: unknown, tc: hegel.TestCase) => unknown };
type Mock = { output: Drawable; of: Record<string, unknown>; name: string };
type Given = { schema: Schema; generator: gs.PrintableGenerator<unknown> };
type Driver = Record<string, unknown> & {
  state: Tagged;
  read: (error?: unknown) => unknown;
  reset: (tc: hegel.TestCase) => unknown;
};
type Served = { file: string; counts: Int32Array; marks: [number, number][] };
type Range = { startOffset: number; endOffset: number; count: number };
type Taken = { result: { url: string; functions: { ranges: Range[] }[] }[] };
type Code = Map<string, Set<number>>;

const SAMPLES = 8;
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
    if (
      script.functions.some(
        ({ ranges: [whole] }) =>
          whole.endOffset > each.counts.length ||
          (whole.startOffset === 0 && whole.endOffset !== each.counts.length)
      )
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

const path = at("WEFT_DRIVER");
const { z } = await import(Bun.resolveSync("zod", dirname(path)));
const registered = new Map<string, Set<string>>();
const add = z.globalRegistry.add.bind(z.globalRegistry);
z.globalRegistry.add = (schema: unknown, ...meta: [{ weft?: unknown } | undefined]) => {
  const prepare = Error.prepareStackTrace;
  Error.prepareStackTrace = (_error, sites) => sites.map((site) => site.getFileName());
  const files = new Error().stack as unknown as (string | null)[];
  Error.prepareStackTrace = prepare;
  const file = files.find((each) => each && SOURCE.test(each));
  const tag = meta[0]?.weft;
  if (file && typeof tag === "string") {
    const from = registered.get(tag) ?? new Set<string>();
    registered.set(tag, from);
    from.add(relative(process.cwd(), file));
  }
  return add(schema, ...meta);
};

const driver: Driver = await import(path);
await post("Profiler.takePreciseCoverage");
const exported = new Map<unknown, string[]>();
for (const [file, each] of served)
  for (const value of Object.values(await import(file)))
    if (typeof value === "function")
      exported.set(value, [...(exported.get(value) ?? []), each.file]);

const tag = (schema: unknown): string | undefined => z.globalRegistry.get(schema)?.weft;
const from = (name: string) => [...(registered.get(name) ?? [])];
const values = Object.values(driver).filter(
  (value): value is Record<string, unknown> => typeof value === "object" && value !== null
);
const given = new Map(
  values
    .filter((value): value is Given => "schema" in value && "generator" in value)
    .map(({ schema, generator }) => [schema, generator])
);
const of = generators((schema) => given.get(schema));
const drawn = (tc: hegel.TestCase, drawable: Drawable) => {
  if (drawable instanceof gs.Generator) return tc.drawSilent(drawable);
  const parsed = accepted(drawable, tc.drawSilent(of(drawable)));
  tc.assume(parsed.length > 0);
  return parsed[0];
};

const paths = (schema: Tagged, at: string[]): [string[], string][] => [
  ...[tag(schema)].flatMap((name) => (name ? [[at, name] as [string[], string]] : [])),
  ...Object.entries(schema._zod.def.shape ?? {}).flatMap(([key, each]) => paths(each, [...at, key])),
];
const reads = paths(driver.state, []);
const state = (raw: unknown): State => {
  const out: State = {};
  for (const [at, name] of reads) {
    const [object, key] = name.split(".");
    const value = at.reduce<unknown>((node, part) => (node as Record<string, unknown>)[part], raw);
    out[object] = { ...out[object], [key]: value as string | number };
  }
  return out;
};
const read = async (...error: unknown[]) => state(driver.state.parse(await driver.read(...error)));

const actions = new Map(
  values
    .filter((value): value is Action => "input" in value && "run" in value)
    .flatMap((action) =>
      (action.input._zod.def.options ?? [action.input]).flatMap((input) => {
        const verb = tag(input);
        return verb ? [[verb, { ...action, input }] as const] : [];
      })
    )
);
const mocks = values
  .filter((value): value is Mock => "output" in value && "of" in value && "name" in value)
  .map((mock) => ({ ...mock, replaced: mock.of[mock.name], spy: spyOn(mock.of, mock.name) }));
const asynchronous = (value: unknown) =>
  Object.prototype.toString.call(value) === "[object AsyncFunction]";
const written = (input: unknown) =>
  Object.entries((input ?? {}) as object)
    .map(([key, value]) => `${key}=${JSON.stringify(value)}`)
    .join(" ");

join(at("WEFT_DIAGRAM"), {
  reads: reads.map(([, name]) => ({ tag: name, from: from(name) })),
  actions: [...actions].map(([verb, { run }]) => ({
    tag: verb,
    from: from(verb),
    run: exported.get(run) ?? [],
  })),
  mocks: mocks.map(({ name, replaced }) => ({ tag: name, run: exported.get(replaced) ?? [] })),
});

const code = { reset: new Map() as Code, actions: new Map<string, Code>() };
const play = {
  reset: (tc: hegel.TestCase) =>
    measured(code.reset, async (): Promise<Report> => {
      for (const { output, replaced, spy } of mocks) {
        const value = drawn(tc, output);
        spy.mockImplementation(asynchronous(replaced) ? async () => value : () => value);
      }
      await driver.reset(tc);
      return { state: await read() };
    }),
  run: async (tc: hegel.TestCase, { run: verb = "" }: Turn): Promise<Report> => {
    const action = actions.get(verb) as Action;
    const raw = tc.drawSilent(of(action.input));
    const input = accepted(action.input, raw);
    if (input.length === 0) return { refused: true };
    const known = code.actions.get(verb) ?? new Map();
    code.actions.set(verb, known);
    return measured(known, async () => {
      const error = await Promise.resolve()
        .then(() => action.run(input[0], tc))
        .then(
          () => [],
          (thrown: unknown) => [thrown]
        );
      return error.length > 0 && driver.read.length === 0
        ? { input: written(raw), thrown: error[0] instanceof Error ? error[0].message : String(error[0]) }
        : { input: written(raw), state: await read(...error) };
    });
  },
  read: async (): Promise<Report> => ({ state: await read() }),
};

let failed = false;
const simulate = () =>
  new hegel.Hegel(async (tc) => {
    failed = false;
    let now = turn({});
    while (!now.done) {
      if (now.failed) {
        failed = true;
        throw new Error(now.failed.join(" · "));
      }
      const kind = (["reset", "run", "read"] as const).find((each) => each in now);
      if (!kind) throw new Error(`weft gives the turn ${JSON.stringify(now)}`);
      now = turn(await play[kind](tc, now));
    }
  })
    .settings({ verbosity: hegel.Verbosity.Quiet, printBlob: false })
    .run()
    .catch((error: unknown) => {
      if (!failed) throw error;
    });

test("weft", async () => {
  const said = await simulate().then(
    () => ({
      ran: {
        reset: spans(code.reset),
        actions: Object.fromEntries([...code.actions].map(([verb, known]) => [verb, spans(known)])),
      },
    }),
    (error: unknown) => ({
      error: error instanceof Error ? (error.stack ?? error.message) : String(error),
    })
  );
  for (const { spy } of mocks) spy.mockRestore();
  done(said);
});
