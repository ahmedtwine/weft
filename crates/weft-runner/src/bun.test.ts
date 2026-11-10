import { plugin } from "bun";
import { spyOn, test } from "bun:test";
import inspector from "node:inspector";
import { createConnection } from "node:net";
import { dirname, relative } from "node:path";
import { fileURLToPath } from "node:url";

type Schema<T = unknown> = { parse: (value: unknown) => T };
type Source = {
  draw: <T>(schema: Schema<T>) => Promise<T>;
};
type Action = { input: Schema; run: (input: unknown) => unknown };
type Mock = { output: Schema; of: Record<string, unknown>; name: string };
type Driver = Record<string, unknown> & {
  state: Schema;
  read: (thrown?: unknown) => unknown;
  reset: (source: Source) => unknown;
  score?: () => unknown;
};
type Node = { [key: string]: unknown };
type Override = {
  zodSchema: { _zod: { def: { type: string } } };
  jsonSchema: Node;
};
type Message = {
  reset?: true;
  apply?: string;
  input?: unknown;
  drawn?: unknown;
  stop?: true;
  ran?: true;
  done?: true;
};
type Served = { file: string; counts: Int32Array; marks: [number, number][] };
type Range = { startOffset: number; endOffset: number; count: number };
type Taken = {
  result: { url: string; functions: { ranges: Range[] }[] }[];
};
type Code = Map<string, Set<number>>;

const SOURCE = new RegExp(
  `^${process.cwd().replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}/(?!.*node_modules/).*\\.[cm]?[jt]sx?$`
);
const BASE64 =
  "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

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
    if (!each || !script.functions.some(({ ranges }) => ranges[0].count > 0))
      continue;
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
      for (const range of ranges)
        each.counts.fill(range.count, range.startOffset, range.endOffset);
    const lines = known.get(each.file) ?? new Set<number>();
    known.set(each.file, lines);
    for (const [offset, line] of each.marks)
      if (each.counts[offset] > 0) lines.add(line);
  }
};

const code = { reset: new Map() as Code, actions: new Map<string, Code>() };

const measured = async <T>(known: Code, run: () => Promise<T>) => {
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
const listed = (tag: unknown): string[] =>
  [tag].flat().filter((each): each is string => typeof each === "string");
const registered = new Map<string, Set<string>>();
const add = z.globalRegistry.add.bind(z.globalRegistry);
z.globalRegistry.add = (schema: unknown, ...meta: [Node | undefined]) => {
  const prepare = Error.prepareStackTrace;
  Error.prepareStackTrace = (_error, sites) =>
    sites.map((site) => site.getFileName());
  const files = new Error().stack as unknown as (string | null)[];
  Error.prepareStackTrace = prepare;
  const file = files.find((each) => each && SOURCE.test(each));
  for (const tag of file ? listed(meta[0]?.weft) : []) {
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
const beside = `${relative(process.cwd(), dirname(path))}/`;
const run = (value: unknown) => {
  const files = exported.get(value) ?? [];
  return files.find((file) => !file.startsWith(beside)) ?? files[0];
};

const from = (node: unknown): unknown => {
  if (Array.isArray(node)) return node.map(from);
  if (typeof node !== "object" || node === null) return node;
  const out: Node = Object.fromEntries(
    Object.entries(node).map(([key, value]) => [key, from(value)])
  );
  const tags = listed(out.weft);
  return tags.length
    ? { ...out, from: tags.flatMap((tag) => [...(registered.get(tag) ?? [])]) }
    : out;
};
const drawable = (schema: Schema) =>
  from(
    z.toJSONSchema(schema, {
      io: "input",
      unrepresentable: "any",
      override: ({ zodSchema, jsonSchema }: Override) => {
        if (zodSchema._zod.def.type === "custom") jsonSchema.not = {};
      },
    })
  ) as Node;
const branches = (node: Node): Node[] => {
  const tagged = [node.oneOf, node.anyOf]
    .flatMap((all): Node[] => (Array.isArray(all) ? all : []))
    .filter((each) => listed(each.weft).length)
    .map((each) => ({ ...each, $defs: node.$defs }));
  return tagged.length ? tagged : [node];
};

const actions = new Map<string, Action>();
const sent: Node[] = [];
const mocks: (Mock & { spy: ReturnType<typeof spyOn>; real: unknown })[] = [];
for (const value of Object.values(driver)) {
  if (typeof value !== "object" || value === null) continue;
  if ("input" in value && "run" in value) {
    const action = value as Action;
    for (const node of branches(drawable(action.input))) {
      sent.push({ ...node, run: run(action.run) });
      for (const verb of listed(node.weft)) actions.set(verb, action);
    }
  }
  if ("output" in value && "of" in value && "name" in value) {
    const mock = value as Mock;
    const real = mock.of[mock.name];
    mocks.push({ ...mock, real, spy: spyOn(mock.of, mock.name) });
  }
}
const refused = ({ input }: Action, value: unknown) => {
  try {
    return { input: input.parse(value) };
  } catch (error) {
    return { refused: error instanceof Error ? error.message : String(error) };
  }
};
const thrown = async (run: () => unknown) => {
  try {
    await run();
    return [];
  } catch (error) {
    if (driver.read.length === 0) throw error;
    return [error];
  }
};
const asynchronous = (value: unknown) =>
  Object.prototype.toString.call(value) === "[object AsyncFunction]";

async function* lines(socket: AsyncIterable<string>) {
  let rest = "";
  for await (const chunk of socket) {
    rest += chunk;
    for (let end = rest.indexOf("\n"); end >= 0; end = rest.indexOf("\n")) {
      yield rest.slice(0, end);
      rest = rest.slice(end + 1);
    }
  }
}

test("weft", async () => {
  const socket = createConnection(at("WEFT_SOCKET")).setEncoding("utf8");
  const incoming = lines(socket);
  const receive = async (): Promise<Message> => {
    const line = await incoming.next();
    if (line.done) throw new Error("weft closes the socket");
    return JSON.parse(line.value);
  };
  const send = (message: unknown) =>
    socket.write(`${JSON.stringify(message)}\n`);
  const drawn = async (schema: unknown) => {
    send({ draw: schema });
    const answer = await receive();
    if (answer.stop) throw new Error("weft stops the step");
    return answer.drawn;
  };
  const source: Source = {
    draw: async (schema) => schema.parse(await drawn(z.toJSONSchema(schema))),
  };
  send({
    state: from(z.toJSONSchema(driver.state)),
    actions: sent,
    mocks: mocks.map(({ output, real }) => ({ ...drawable(output), run: run(real) })),
  });
  const step = async (message: Message) => {
    if (message.ran)
      return {
        ran: {
          reset: spans(code.reset),
          actions: Object.fromEntries(
            [...code.actions].map(([verb, known]) => [verb, spans(known)])
          ),
        },
      };
    const action = actions.get(message.apply ?? "");
    if (!message.reset && !action)
      throw new Error(`no action has the tag ${message.apply}`);
    const known: Code = message.reset
      ? code.reset
      : (code.actions.get(message.apply ?? "") ?? new Map());
    if (action) code.actions.set(message.apply ?? "", known);
    return measured(known, async () => {
      if (message.reset) {
        for (const { output, real, spy } of mocks) {
          const value = await source.draw(output);
          spy.mockImplementation(
            asynchronous(real) ? async () => value : () => value
          );
        }
        await driver.reset(source);
      }
      const parsed = action ? refused(action, message.input) : {};
      const error =
        action && "input" in parsed
          ? await thrown(() => action.run(parsed.input))
          : [];
      const state = driver.state.parse(await driver.read(...error));
      const end = "refused" in parsed ? { refused: parsed.refused } : {};
      return driver.score
        ? { state, ...end, score: await driver.score() }
        : { state, ...end };
    });
  };
  for (let message = await receive(); !message.done; message = await receive()) {
    send(
      await step(message).catch((error: unknown) => ({
        error: error instanceof Error ? (error.stack ?? error.message) : String(error),
      }))
    );
  }
  socket.end();
});
