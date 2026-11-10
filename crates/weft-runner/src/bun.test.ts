import { test } from "bun:test";
import { createConnection } from "node:net";
import { dirname } from "node:path";

type Schema = {
  meta: () => { weft?: string } | undefined;
  parse: (value: unknown) => unknown;
};
type Action = {
  input: Schema;
  run: (object: string, input: unknown) => unknown;
};
type Driver = Record<string, unknown> & {
  state: Schema;
  read: () => unknown;
  reset: () => unknown;
};
type Message = {
  reset?: true;
  apply?: string;
  object?: string;
  input?: unknown;
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
  const send = (message: unknown) =>
    socket.write(`${JSON.stringify(message)}\n`);
  send({
    state: z.toJSONSchema(driver.state),
    actions: [...actions.values()].map((action) =>
      z.toJSONSchema(action.input)
    ),
  });
  const step = async (message: Message) => {
    const action = actions.get(message.apply);
    if (message.reset) await driver.reset();
    if (!message.reset && !action)
      throw new Error(`no action has the tag ${message.apply}`);
    if (action) await action.run(message.object ?? "", action.input.parse(message.input));
    return { state: driver.state.parse(await driver.read()) };
  };
  for await (const line of lines(socket)) {
    const message: Message = JSON.parse(line);
    if (message.done) break;
    send(
      await step(message).catch((error: unknown) => ({
        error: error instanceof Error ? (error.stack ?? error.message) : String(error),
      }))
    );
  }
  socket.end();
});
