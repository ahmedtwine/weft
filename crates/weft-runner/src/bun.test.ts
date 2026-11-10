import { plugin } from "bun";
import { test } from "bun:test";
import inspector from "node:inspector";
import { createConnection } from "node:net";
import { dirname, relative } from "node:path";
import { fileURLToPath } from "node:url";

type Schema<T = unknown> = {
  meta: () => { weft?: string } | undefined;
  parse: (value: unknown) => T;
};
type Source = {
  draw: <T>(schema: Schema<T>) => Promise<T>;
};
type Action = {
  input: Schema;
  run: (object: string, input: unknown, source: Source) => unknown;
};
type Driver = Record<string, unknown> & {
  state: Schema;
  read: () => unknown;
  reset: (source: Source) => unknown;
  score?: () => unknown;
};
type Message = {
  reset?: true;
  apply?: string;
  object?: string;
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
const driver: Driver = await import(path);
await post("Profiler.takePreciseCoverage");
const { z } = await import(Bun.resolveSync("zod", dirname(path)));
const actions = new Map(
  Object.values(driver)
    .filter(
      (value): value is Action =>
        typeof value === "object" &&
        value !== null &&
        "input" in value &&
        "run" in value
    )
    .map((action) => [action.input.meta()?.weft, action])
);

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
    state: z.toJSONSchema(driver.state),
    actions: [...actions.values()].map((action) =>
      z.toJSONSchema(action.input)
    ),
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
    const action = actions.get(message.apply);
    if (!message.reset && !action)
      throw new Error(`no action has the tag ${message.apply}`);
    const known: Code = message.reset
      ? code.reset
      : (code.actions.get(message.apply ?? "") ?? new Map());
    if (action) code.actions.set(message.apply ?? "", known);
    return measured(known, async () => {
      if (message.reset) await driver.reset(source);
      if (action)
        await action.run(message.object ?? "", action.input.parse(message.input), source);
      const state = driver.state.parse(await driver.read());
      return driver.score ? { state, score: await driver.score() } : { state };
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
