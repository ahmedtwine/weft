# weft

Invariant diagrams in [KDL](https://kdl.dev): objects with state machines, actions, and checks, laid out by [merman](https://github.com/Latias94/merman) in Pele styling.

Write each object as `<kind> <name>`. The kind is `person`, `element`, `browser`, `db`, `api` or `file`. A `person` has no states. Each other object sets its start state with `initial=` and holds its states, and each state holds its transitions, as in [SCXML](https://www.w3.org/TR/scxml/):

```kdl
element box initial=checked {
  state checked {
    on uncheck unchecked
  }
  state unchecked {
    on check checked
  }
}
```

`on <verb> <state>` moves the object from the state that holds it to `<state>`. A state that holds states sets its start state with `initial=`, and its transitions hold for each state inside it. Each state is reached from `initial=` by the `action`, and no two transitions of one verb start from the same state. Names and states are KDL strings.

Each `action` line `<verb> from=<object> to=<object>` moves its `to=` object through its transition for the verb. Only the last line sets a state, as `<object>=<state>`, and only for the objects of `forall`: that state breaks the check.

A root `weft.kdl` holds `include "**/*.weft.kdl"`, `theme light` or `theme dark`, and one `style <kind> icon=<lucide-icon>` line for each kind that the files use. `weft` parses `weft.kdl` first, and each file then takes only the kinds that have a `style` line. Then run `weft` to check every file, or `weft --ui` to open the report on a local server.

`check` holds one `invariant` in one shape of the [Quint builtins](https://quint.sh/docs/builtin):

```kdl
invariant {
  forall <kind> <object>… {
    implies {
      in step <verb>…
      eq (next)<kind> <kind>
    }
  }
}
```

The objects of `forall` are of `<kind>` and have no transition for a verb of `in step`. Each verb is the verb of a transition. A state of each object can replace the last `<kind>`. The report writes the statement from the condition, and shows the states that break it, as `eq("<state after>", "<state before>") = false`.

MIT. merman is MIT OR Apache-2.0. Icons are Lucide, ISC, in `crates/weft-draw/src/vendor`.
