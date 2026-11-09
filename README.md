# weft

Invariant diagrams in [KDL](https://kdl.dev): objects with state machines, steps that carry nested payloads, and checks, laid out by [merman](https://github.com/Latias94/merman) in Pele styling.

Write each object as `<kind> <name> "<label>"`. The kind is `person`, `element`, `browser`, `db`, `api` or `file`. A root `weft.kdl` holds `include "**/*.weft.kdl"`, `theme light` or `theme dark`, and one `style <kind> icon=<lucide-icon>` line for each kind that the files use. `weft` parses `weft.kdl` first, and each file then takes only the kinds that have a `style` line. Then run `weft` to check every file, or `weft --ui` to open the report on a local server.

`check` holds one `invariant` in one shape of the [Quint builtins](https://quint.sh/docs/builtin):

```kdl
invariant {
  forall <name> <object>… {
    implies {
      in step <verb>…
      eq (next)<name> <name>
    }
  }
}
```

Each object has states and `value=<noun>`, and no `on` line for a verb of `in step`. Each verb is the verb of an `on` line. A state of each object can replace the last `<name>`. The report writes the statement from the condition.

MIT. merman is MIT OR Apache-2.0. Icons are Lucide, ISC, in `crates/weft-draw/src/vendor`.
