#!/usr/bin/env bun
import { join } from "node:path";
import { version } from "../package.json";

const target = `${process.platform}-${process.arch}`;
const dir = join(import.meta.dir, "..", "dist", `v${version}`, target);
const exe = join(dir, "weft");
if (!(await Bun.file(exe).exists())) {
  const url = `https://github.com/ahmedtwine/weft/releases/download/v${version}/weft-${target}.tar.gz`;
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: ${response.status} ${response.statusText}`);
  await Bun.$`mkdir -p ${dir} && tar -xzf - -C ${dir} < ${await response.blob()}`;
}
const run = Bun.spawn([exe, ...Bun.argv.slice(2)], { stdio: ["inherit", "inherit", "inherit"] });
process.exit(await run.exited);
