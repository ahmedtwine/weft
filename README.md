# weft

A small language and test runner for invariant diagrams. A diagram shows the
objects, the reads and writes between them in time order, the state of each
object after a write, and the step where an invariant breaks.

```sh
cargo install --git https://github.com/ahmedtwine/weft weft-cli --locked
weft docs/invariants
weft docs/invariants --visualize
weft docs/invariants --visualize --format html > report.html
```

## Language

```text
title "I5 · An aborted request writes nothing"

objects {
  box      "search box\n(page)"
  requestA "request A\nPOST /api/correlation"
  title736 "title 736\n(page)"
}

init {
  title736' = masked
}

run {
  box -> requestA: "coffee"
  box -> requestA: abort
  requestA' = aborted
  requestA -> title736: "Coffee maker"
  title736' = "Coffee maker"
}

check "An aborted request writes nothing" {
  from requestA => requestA != "aborted"
}
```

| Part | Meaning |
| --- | --- |
| `objects { name "Label" }` | One lifeline for each object. `\n` breaks a label. |
| `init { x' = v }` | The state before the first step. |
| `run { a -> b: text }` | One step: `a` reads or writes `b`, carrying `text`. |
| `x' = v` | The state of `x` after the step above. `'` is the only temporal operator. |
| `check "Title" { p => q }` | The invariant, checked after each step. `=> q` is optional. |
| `x`, `x'`, `"v"` | The state before the step, the state after it, a literal. |
| `=`, `!=` | Equal, not equal. |
| `from x` | The step starts at `x`. |

## Runner

A `run` is a counterexample. The runner passes a file when its run breaks its
check, and the diagram marks that step in red. It fails a file when the run
does not break the check, or when the file does not parse. The exit status is
0 when every file passes, 1 when a file fails, and 2 for a usage error.

| Flag | Output |
| --- | --- |
| none | One line for each file. |
| `--visualize` | Each diagram as text. |
| `--visualize --format html` | One HTML page with an SVG for each diagram, to stdout. |

## Crates

| Crate | Job |
| --- | --- |
| `weft-parse` | The `nom` parser and the check of the run. |
| `weft-draw` | The layout, and the SVG and text output. |
| `weft-cli` | The `weft` test runner. |

## Sources

- Hypothesis and Hegel: setup, random actions, a check after each action,
  and the shortest failing run. A generator crate will make runs from rules.
- Quint and TLA+: `init`, `x'` for the next state, and `=>`.
- Mermaid and merman: objects as lifelines, arrows with text, notes, step
  numbers, and column gaps that grow to fit the text of each arrow. The
  Helvetica widths come from the font metrics.
