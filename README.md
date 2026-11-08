# weft

A small language and test runner for invariant diagrams. One diagram type
shows typed objects, the reads and writes between them in time order, the
state of each object after a write, the state machine of an object, and the
step where an invariant breaks. [merman](https://github.com/Latias94/merman)
lays out and draws each diagram.

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
  page   box "search box" on "results page"
  code   resolution "content-resolution.ts"
  api    requestA POST "/api/correlation" {
    open -> aborted
    open -> streamed
  }
  page   title736 "title 736" on "results page"
}

init {
  requestA' = open
}

run {
  resolution -> requestA: abort
  requestA' = aborted
  requestA -> resolution: data-text {nodeId: 736, value: "Coffee maker"}
  requestA' = streamed
}
```

### Objects

Each object has a kind and the typed properties of that kind.

| Kind | Properties | Example | Shape |
| --- | --- | --- | --- |
| `person` | name | `person agent "Agent"` | actor |
| `api` | `GET`, `POST`, `PUT`, `PATCH` or `DELETE`, and a path | `api run POST "/api/agents/[id]/run"` | boundary |
| `page` | an element `on` a page | `page title "title 736" on "results page"` | box |
| `code` | a symbol | `code planner "interaction-planner.ts"` | control |
| `db` | a table and a row key | `db row counterfactuals "R1"` | database |
| `file` | `S3` or `disk`, and a path | `file map S3 "element-map.json"` | collections |
| `browser` | `history.state`, `localStorage`, `sessionStorage` or `cookie` | `browser history history.state` | entity |

An object can declare its state machine in braces: `a -> b` on each line.

### Blocks and operators

| Part | Meaning |
| --- | --- |
| `init { x' = v }` | The state before the first step. |
| `run { a -> b: text }` | One step: `a` reads or writes `b`, carrying `text`. |
| `x' = v` | The state of `x` after the step above. `'` is the only temporal operator. |
| `Caption {key: value, …}` | A data value. Each field shows on its own line, cut at 24 characters. |
| `check "Title" { p => q }` | The invariant, checked after each step. `=> q` is optional. |
| `x`, `x'`, `"v"` | The state before the step, the state after it, a literal. |
| `=`, `!=` | Equal, not equal. |
| `from x` | The step starts at `x`. |

A step breaks the diagram when it breaks the check, or when it changes the
state of an object to a state that its state machine does not allow.

## Runner

A `run` is a counterexample. The runner passes a file when its run breaks
its check or a state machine, and the diagram marks that step in red. It
fails a file when nothing breaks, or when the file does not parse. The exit
status is 0 when every file passes, 1 when a file fails, and 2 for a usage
error.

| Flag | Output |
| --- | --- |
| none | One line for each file. |
| `--visualize` | Each diagram as text. |
| `--visualize --format html` | One HTML page with an SVG for each diagram, to stdout. |

## Crates

| Crate | Job |
| --- | --- |
| `weft-parse` | The `nom` parser, the typed objects and the check of the run. |
| `weft-draw` | Turns a diagram into a Mermaid sequence diagram that merman draws as SVG or text. |
| `weft-cli` | The `weft` test runner. |
| `weft-c` | `libweft`, the C ABI for SDKs, with the header `include/weft.h`. |

## C ABI

`libweft` follows the calling convention of libhegel. Each call takes a
`weft_context_t*` and returns a `weft_result_t`; `WEFT_OK` is zero and each
error is negative. Strings are `{data, len}`, and each string that libweft
returns is released with `weft_owned_string_free`. A Node or Bun SDK loads
the library with [koffi](https://koffi.dev).

```sh
cargo build --release -p weft-c
WEFT_C_HEADER_WRITE=1 cargo test -p weft-c
```

## Sources

- Hypothesis and Hegel: setup, actions, a check after each action, and the
  C ABI of libhegel.
- Quint and TLA+: `init`, `x'` for the next state, and `=>`.
- Mermaid and merman: the sequence layout, the participant shapes, the notes,
  the step numbers and the text output.

## License

MIT. weft depends on merman, which is MIT OR Apache-2.0, with only its
sequence diagram, SVG and text features. Those features do not include the
EPL-2.0 ELK layout. merman uses `cssparser` and `dtoa-short`, which are
MPL-2.0 and used without change. `cbindgen`, also MPL-2.0, is only a build
tool for the header. merman is independent of the Mermaid project; see its
[third-party notices](https://github.com/Latias94/merman/blob/main/THIRD_PARTY_NOTICES.md).
