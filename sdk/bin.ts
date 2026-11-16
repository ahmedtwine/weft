#!/usr/bin/env bun
import { join } from "node:path";
import { dist } from "./dist";

const run = Bun.spawn([join(await dist(), "weft"), ...Bun.argv.slice(2)], { stdio: ["inherit", "inherit", "inherit"] });
process.exit(await run.exited);
