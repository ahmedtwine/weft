import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SHIPPED = ("weft-cli", "weft-c", "weft-sdk")
OTHERS = [
    ("Lucide", "ISC", "https://lucide.dev", ROOT / "crates/weft-draw/src/vendor/LICENSE-lucide"),
    ("koffi", "MIT", "https://koffi.dev", ROOT / "node_modules/koffi/LICENSE.txt"),
    ("zod", "MIT", "https://zod.dev", None),
    ("Perspective", "Apache-2.0", "https://perspective-dev.github.io", ROOT / "crates/weft-dashboard/site/vendor/client/LICENSE.md"),
    ("pele", "MIT", "https://pele.run", ROOT / "crates/weft-dashboard/site/vendor/pele/LICENSE"),
    ("ELK", "EPL-2.0", "https://eclipse.dev/elk", ROOT / "crates/weft-dashboard/site/vendor/elk/LICENSE.md"),
]


def crates():
    meta = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--locked"], cwd=ROOT))
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    members = set(meta["workspace_members"])
    stack = [i for i in members if packages[i]["name"] in SHIPPED]
    seen = set()
    while stack:
        at = stack.pop()
        if at in seen:
            continue
        seen.add(at)
        stack.extend(
            d["pkg"] for d in nodes[at]["deps"] if any(k["kind"] in (None, "build") for k in d["dep_kinds"])
        )
    return sorted((packages[i] for i in seen - members), key=lambda p: (p["name"], p["version"]))


def texts(package):
    folder = Path(package["manifest_path"]).parent
    found = sorted(f for f in folder.iterdir() if f.is_file() and f.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE")))
    kept = ROOT / "scripts/licenses" / f"{package['name']}.txt"
    found = found or ([kept] if kept.exists() else [])
    return [f.read_text(errors="replace").strip() for f in found]


def main():
    out = ["# Third-party notices", "", "weft ships the code below. Each part keeps its own license.", ""]
    out += ["| Name | Version | License | Source |", "| ---- | ------- | ------- | ------ |"]
    shipped = crates()
    for p in shipped:
        out.append(f"| {p['name']} | {p['version']} | {p['license']} | {p.get('repository') or 'https://crates.io/crates/' + p['name']} |")
    for name, license, url, _ in OTHERS:
        out.append(f"| {name} | | {license} | {url} |")
    out += ["", "## License texts", ""]
    seen = {}
    for p in shipped:
        for text in texts(p):
            seen.setdefault(text, []).append(f"{p['name']} {p['version']}")
    for name, _, _, path in OTHERS:
        if path is not None and path.exists():
            seen.setdefault(path.read_text().strip(), []).append(name)
    for text, owners in seen.items():
        out += [f"### {', '.join(owners)}", "", "```text", text, "```", ""]
    (ROOT / "THIRD-PARTY-NOTICES.md").write_text("\n".join(out))


main()
