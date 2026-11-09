# weft

Invariant diagrams in [KDL](https://kdl.dev): objects with state machines, steps that carry nested payloads, and checks, laid out by [merman](https://github.com/Latias94/merman) in Pele styling.

Write each object as `<kind> <name> "<label>"`. You choose the kinds. A root `weft.kdl` holds `include "**/*.weft.kdl"`, `theme light` or `theme dark`, and one `style <kind> icon=<lucide-icon>` line for each kind. Then run `weft` to check every file, or `weft --ui` to open the report on a local server.

`check` holds one `invariant <Name>` with one condition of the [Quint builtins](https://quint.sh/docs/builtin) `forall`, `implies`, `iff`, `and`, `eq` and `in`. A value is an object, one of its states, `step`, or `(next)<object>`. The report writes the statement from the condition. The run must break the invariant at its last step.

MIT. merman is MIT OR Apache-2.0. Icons are Lucide, ISC, in `crates/weft-draw/src/vendor`.
