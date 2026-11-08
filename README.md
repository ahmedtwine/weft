# weft

Invariant diagrams in [KDL](https://kdl.dev): objects with state machines, steps that carry nested payloads, and checks, laid out by [merman](https://github.com/Latias94/merman) in Pele styling.

Write each object as `<kind> <name> "<label>"`. You choose the kinds. A root `weft.kdl` holds `include "**/*.weft.kdl"`, `theme light` or `theme dark`, and one `style <kind> icon=<lucide-icon>` line for each kind. Then run `weft [--visualize] [--format ascii|html]`.

MIT. merman is MIT OR Apache-2.0. Icons are Lucide, ISC, in `crates/weft-draw/src/vendor`.
