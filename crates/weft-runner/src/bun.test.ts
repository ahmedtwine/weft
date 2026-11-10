import { test } from "bun:test";
import { createConnection } from "node:net";
import { dirname } from "node:path";

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
  done?: true;
};

const at = (name: string) => {
  const value = Bun.env[name];
  if (!value) throw new Error(`${name} is not set`);
  return value;
};
const path = at("WEFT_DRIVER");
const driver: Driver = await import(path);
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
    const action = actions.get(message.apply);
    if (message.reset) await driver.reset(source);
    if (!message.reset && !action)
      throw new Error(`no action has the tag ${message.apply}`);
    if (action)
      await action.run(message.object ?? "", action.input.parse(message.input), source);
    const state = driver.state.parse(await driver.read());
    return driver.score ? { state, score: await driver.score() } : { state };
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
