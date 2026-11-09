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

## Simulate

```
weft.kdl: adapter bun cases=100
    │
weft ── parse <name>.weft.kdl ── run ── print the run tables
    │
weft-simulator
    ├── Tags     the JSON Schema of the state and of each input, read by tag
    ├── Hegel    one stateful rule for each step: a transition, an input, a shrink
    ├── Check    the condition on the state before and after each step
    └── Runner   schema · reset · apply
          └── Bun    bun test <harness> ── socket, JSON lines ── <name>.weft.ts ── the code
```

`adapter <name> cases=<count>` in `weft.kdl` makes `weft` run each file against the code. `weft --ui` does not run the code. The crate `weft-simulator` holds the run. A runner is the `Runner` trait: `schema` gives the JSON Schema of the state and of each action, `reset` starts the code and gives its state, and `apply` runs one action and gives the new state. `adapter bun` is the runner that `weft` has now.

Each `<name>.weft.kdl` has a driver `<name>.weft.ts` beside it, with these exports:

```ts
export const state = z.strictObject({
  page: z.strictObject({ count: z.number().int().meta({ weft: "page.count" }) }),
});
export const read = () => ({ page: { count: rows().length } });
export const reset = () => { … };
export const send = {
  input: z.strictObject({ text: z.string() }).meta({ weft: "send" }),
  run: (object: string, input: { text: string }) => { … },
};
```

A tag is `.meta({ weft: "<object>.<key>" })` on a field of the state, or a list of tags, so that one field of the code can carry the tags of more than one file. A file takes only the tags of its own objects. Each action is an object with `input` and `run`, and the tag of its `input` is its verb. The tags do not change how Zod parses.

`weft` writes its harness to a temporary directory and runs `bun test` on it in the directory of `weft.kdl`, so the preload and the module resolution of the app apply. The harness loads the driver, sends the JSON Schema of `state` and of each `input` through `z.toJSONSchema`, and then calls `reset`, `run` and `read` for each message from `weft`.

Before the run, `weft` checks that each `<object>.<key>` of the check has a tag, that each tag names a key of its object, that each verb has one action, and that each action is a verb of the file. Each case is a [Hegel](https://hegel.dev) state machine of up to 20 steps. In each step, Hegel draws a transition of the setup that starts from the state of the model, and a value for each field of its input: a string, an integer or a boolean, in the bounds of its JSON Schema. After each step, `weft` reads the state from the code and checks the condition on the state before and after the step. A throw of the code fails the step. Hegel shrinks a failing case to the fewest steps and the smallest values.

`weft` prints the run, a table of the steps of the failing case with the input, the state and the values that break the check, and a table of each action with its steps and the steps where the check applies. It gives `✕` for a failing case.

MIT. merman is MIT OR Apache-2.0. Icons are Lucide, ISC, in `crates/weft-draw/src/vendor`.
