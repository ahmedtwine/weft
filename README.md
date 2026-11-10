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
    ├── Engine   runs each case: start, then up to 20 steps
    │     ├── hegel   one stateful rule for each step, a target, a shrink
    │     └── random  seeded draws, no shrink
    ├── Source   draw · pick · target, from the engine to the run and the runner
    ├── Check    the condition on the state before and after each step
    └── Runner   schema · reset · apply · check
weft-runner
    └── Adapter  one runner for each adapter line
          ├── Default  the transitions of the setup, in Rust
          └── Wire     JSON messages to a runner and its answers
                ├── bun   bun test <harness> ── unix socket ── <name>.weft.ts ── the code
                ├── unix  a runner that listens on a unix socket
                └── http  a runner at a URL
```

`weft` runs each file. `weft --ui` does not run. The crate `weft-simulator` holds the run. An engine is the `Engine` trait: it runs each case through the `Case` trait, `start` and then each `step`, and gives each of them a `Source`. A source is the `Source` trait: `draw` gives a value for a field of a JSON Schema, `pick` gives an index into a list, and `target` gives a score. The engine keeps the highest score of a case. `engine hegel` is the default. `engine random seed=<number>` draws from a seeded generator and does not shrink. A runner is the `Runner` trait: `schema` gives the JSON Schema of the state and of each action, `reset` starts the run and gives its state, `apply` runs one action and gives the new state, and `check` checks the condition on the state before and after a step. The crate `weft-runner` holds `Adapter`, one enum with a runner for each adapter line.

Without an `adapter` line, the runner is `adapter default cases=100`. It runs the transitions of the setup in Rust, without a driver and without the code, and its tables show how many steps and checks each action gets. These lines in `weft.kdl` run each file against the code:

| Line | Runner |
| --- | --- |
| `adapter bun cases=<count>` | `weft` starts `bun test` on its harness, and the harness connects to `weft` on a unix socket |
| `adapter unix path=<socket> cases=<count>` | `weft` connects to a runner that listens on the unix socket |
| `adapter http url=<url> cases=<count>` | `weft` sends `GET <url>` for the schema and `POST <url>` for each message |

Each runner gets the same JSON messages and gives the same answers. On a unix socket, each message and each answer is one line, and the first line from the runner is the schema.

| Message from `weft` | Answer of the runner |
| --- | --- |
| `{"reset": true}` | `{"state": <state>}` or `{"error": "<text>"}` |
| `{"apply": "<verb>", "object": "<object>", "input": <input>}` | `{"state": <state>}` or `{"error": "<text>"}` |
| `{"drawn": <value>}` | the next answer of the same step |
| `{"stop": true}` | any answer; `weft` drops it |
| `{"done": true}` | none; the run ends |

The schema is `{"state": <JSON Schema>, "actions": [<JSON Schema>, …]}`. An `error` answer is a throw of the code and fails the step. Before it gives the state of a step, a runner can answer `{"draw": <JSON Schema>}` with the schema of one string, integer or boolean. `weft` draws the value from the engine and sends `{"drawn": <value>}`, so the engine records and shrinks each value that the runner draws. The run table shows these values as `drawn=<value>,…` in the input of the step. When the engine ends a case during a draw, `weft` sends `{"stop": true}` in place of `{"drawn": …}` before its next message, and the runner ends the step. A `state` answer can also carry `"score": <number>`, and `weft` gives it to the engine as a target.

With `adapter bun`, each `<name>.weft.kdl` has a driver `<name>.weft.ts` beside it, with these exports:

```ts
export const state = z.strictObject({
  page: z.strictObject({ count: z.number().int().meta({ weft: "page.count" }) }),
});
export const read = () => ({ page: { count: rows().length } });
export const reset = (draw: Draw) => { … };
export const score = () => added;
export const send = {
  input: z.strictObject({ text: z.string() }).meta({ weft: "send" }),
  run: async (object: string, input: { text: string }, draw: Draw) => {
    const amount = await draw(z.number().int().min(0).max(9));
    …
  },
};
```

A tag is `.meta({ weft: "<object>.<key>" })` on a field of the state, or a list of tags, so that one field of the code can carry the tags of more than one file. A file takes only the tags of its own objects. Each action is an object with `input` and `run`, and the tag of its `input` is its verb. The tags do not change how Zod parses.

`weft` writes its harness to a temporary directory and runs `bun test` on it in the directory of `weft.kdl`, so the preload and the module resolution of the app apply. The harness loads the driver, sends the JSON Schema of `state` and of each `input` through `z.toJSONSchema`, and then calls `reset`, `run` and `read` for each message from `weft`. `draw(<Zod schema>)` sends the JSON Schema of one string, integer or boolean to `weft` and gives the value that the engine draws, parsed by the schema. The export `score` is optional: when the driver has it, the harness sends its value with each state.

Before the run, `weft` checks that each `<object>.<key>` of the check has a tag, that each tag names a key of its object, that each verb has one action, and that each action is a verb of the file. Each case is a [Hegel](https://hegel.dev) state machine of up to 20 steps. In each step, Hegel draws a transition of the setup that starts from the state of the model, and a value for each field of its input: a string, an integer or a boolean, in the bounds of its JSON Schema. After each step, `weft` reads the state from the code and checks the condition on the state before and after the step. A throw of the code fails the step. Hegel shrinks a failing case to the fewest steps and the smallest values.

`weft` prints the run, a table of the steps of the failing case with the input, the state and the values that break the check, and a table of each action with its steps and the steps where the check applies. It gives `✕` for a failing case.

MIT. merman is MIT OR Apache-2.0. Icons are Lucide, ISC, in `crates/weft-draw/src/vendor`.
