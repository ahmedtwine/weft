import { join } from "node:path";
import { dist } from "./dist";
import { ended, setenv, started } from "./weft";

const PARENT = "--parent";
const ADAPTER = "bun";
const LIBRARY = process.platform === "darwin" ? "libweft.dylib" : "libweft.so";

type Joined = { cwd: string; env: Record<string, string> };

const joined = async ({ cwd, env }: Joined) => {
  process.chdir(cwd);
  setenv(env);
  const { run } = await import("./runner");
  return run().then(
    () => undefined,
    (error: unknown) => error
  );
};

const spawned = async (exe: string, file: string) => {
  const { stdin, stdout, exited } = Bun.spawn([exe, PARENT, ADAPTER, file], {
    stdin: "pipe",
    stdout: "pipe",
    stderr: "inherit",
  });
  const reader = stdout.getReader();
  const decoder = new TextDecoder();
  let said = "";
  const more = async () => {
    const { done, value } = await reader.read();
    said += decoder.decode(value, { stream: true });
    return !done;
  };
  while (!said.includes("\n") && (await more()));
  const [first] = said.split("\n", 1);
  const joins = first.startsWith("{");
  const thrown = joins ? await joined(JSON.parse(first)) : undefined;
  while (await more());
  const report = joins ? said.slice(first.length + 1) : said;
  const code = await exited;
  stdin.end();
  if (code !== 0) throw new Error(report || `weft ends with ${code}`);
  if (thrown) throw thrown;
  console.log(report);
};

const threaded = async (file: string) => {
  const thrown = await joined(started(join(await dist(), LIBRARY), ADAPTER, file));
  const { failed, report } = ended();
  if (failed) throw new Error(report);
  if (thrown) throw thrown;
  console.log(report);
};

export const weft = (file: string) => {
  const exe = Bun.env.WEFT_BIN;
  return exe ? spawned(exe, file) : threaded(file);
};
