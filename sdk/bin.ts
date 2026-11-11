#!/usr/bin/env bun
import { join } from "node:path";

const exe = join(import.meta.dir, "..", "target", "release", "weft");
if (!(await Bun.file(exe).exists()))
  throw new Error(
    `${exe} is not built: run cargo build --release --locked --manifest-path ${join(import.meta.dir, "..", "Cargo.toml")} -p weft-cli -p weft-c`
  );
const run = Bun.spawn([exe, ...Bun.argv.slice(2)], { stdio: ["inherit", "inherit", "inherit"] });
process.exit(await run.exited);
