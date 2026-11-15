#!/usr/bin/env bun
import { join } from "node:path";
import { version } from "../package.json";

const target = `${process.platform}-${process.arch}`;
const dir = join(import.meta.dir, "..", "dist", `v${version}`, target);
const exe = join(dir, "weft");
if (!(await Bun.file(exe).exists())) {
  const token = process.env.GH_TOKEN;
  if (!token) throw new Error("GH_TOKEN must give read access to ahmedtwine/weft");
  const get = async (url: string, accept: string) => {
    const response = await fetch(url, { headers: { authorization: `Bearer ${token}`, accept } });
    if (!response.ok) throw new Error(`${url}: ${response.status} ${response.statusText}`);
    return response;
  };
  const release = await get(
    `https://api.github.com/repos/ahmedtwine/weft/releases/tags/v${version}`,
    "application/vnd.github+json",
  );
  const { assets }: { assets: { name: string; url: string }[] } = await release.json();
  const asset = assets.find(({ name }) => name === `weft-${target}.tar.gz`);
  if (!asset) throw new Error(`v${version} has no weft-${target}.tar.gz`);
  const archive = await get(asset.url, "application/octet-stream");
  await Bun.$`mkdir -p ${dir} && tar -xzf - -C ${dir} < ${await archive.blob()}`;
}
const run = Bun.spawn([exe, ...Bun.argv.slice(2)], { stdio: ["inherit", "inherit", "inherit"] });
process.exit(await run.exited);
