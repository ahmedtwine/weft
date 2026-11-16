import { setenv } from "./weft";

const PARENT = "--parent";
const ADAPTER = "bun";

export const weft = async (file: string) => {
  const exe = Bun.env.WEFT_BIN ?? "weft";
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
  if (joins) {
    const { cwd, env } = JSON.parse(first);
    process.chdir(cwd);
    setenv(env);
    const { run } = await import("./runner");
    await run();
  }
  while (await more());
  const report = joins ? said.slice(first.length + 1) : said;
  const code = await exited;
  stdin.end();
  if (code !== 0) throw new Error(report || `weft ends with ${code}`);
  console.log(report);
};
