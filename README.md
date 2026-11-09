# weft

Invariant diagrams in [KDL](https://kdl.dev): objects with state machines, actions, and checks, laid out by [merman](https://github.com/Latias94/merman) in Pele styling.

Write each object as `<kind> <name> <key>=<value>…`. The kind is `person`, `element`, `browser`, `db`, `api` or `file`. A `person` has no states. Each other object sets its start state on its line, and holds its states, and each state holds its transitions, as in [SCXML](https://www.w3.org/TR/scxml/):

```kdl
element box checked="true" {
  state checked="true" {
    on uncheck checked="false"
  }
  state checked="false" {
    on check checked="true"
  }
}
```

Each state is `<key>=<value>…`, with each key of the start state. A value is a KDL string or integer, and each key keeps the type of its start value. `on <verb> <key>=<value>…` moves the object from the state that holds it to the state with these values. No two transitions of one verb start from the same state. Names are KDL strings.

Each `action` line `<verb> from=<object> to=<object>` moves its `to=` object through its transition for the verb. The `action` lines reach each object and each state. Only the last line sets a state, as `<object>.<key>=<value>`, and only for the objects of `forall`: that state breaks the check, and it need not be a declared state.

A root `weft.kdl` holds `include "**/*.weft.kdl"`, `theme light` or `theme dark`, and one `style <kind> icon=<lucide-icon>` line for each kind that the files use. `weft` parses `weft.kdl` first, and each file then takes only the kinds that have a `style` line. Then run `weft` to check every file, or `weft --ui` to open the report on a local server.

`check` holds one `invariant` as a tree of [Quint builtins](https://quint.sh/docs/builtin):

```kdl
invariant {
  forall <kind> <object>… {
    <condition>
  }
}
```

A condition is `and { <condition>… }`, `implies { <condition> <condition> }`, `in step <verb>…` or `eq <term> <term>`. A term is `<kind>.<key>`, `(next)<kind>.<key>`, a value, or a child line `iadd <term> <term>` of two integers. Each `eq` names at least one `<kind>.<key>`, and takes two terms of one type. The objects of `forall` are of `<kind>` and have each `<key>`, and each verb of `in step` is the verb of a transition. A condition without `next` holds in each state, as a Quint invariant. A condition with `next` holds at each step, from the state before it to the state after it. `weft` checks the condition in the start state and at each step from it, by each transition of the setup, and then at each `action` line:

```kdl
invariant {
  forall browser page {
    and {
      implies {
        in step send
        eq (next)browser.count {
          iadd browser.count 1
        }
      }
      implies {
        in step open reload
        eq (next)browser.count browser.count
      }
    }
  }
}
```

The report heads each check with this condition, and with a sentence that each operator of the condition writes: `always` as "always", `forall` as "for each … in …", `and` as ", and", `implies` as "if …, then …", `in` as "is … or …", `eq(next(x), x)` as "x is unchanged", `eq(next(x), v)` as "x becomes v", any other `eq` as "equals", and `iadd(a, b)` as "a + b". It shows the values that break it, as `eq(<value>, <value>) = false`. The diagram draws a «check» box over each object of `forall` at each step where the check applies, with the state of the object after the step. At the step that breaks the check, the box adds the values that break it, as `eq(<value>, <value>) = false`.

MIT. merman is MIT OR Apache-2.0. Icons are Lucide, ISC, in `crates/weft-draw/src/vendor`.
