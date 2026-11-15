# invariants

You mark a part of any artifact with a note. A model writes the invariants that can explain your note, with one alternate of the part for each invariant. Your reactions to the alternates leave the weakest invariant that the part violates, and you lint new content with it.

## Types

| Type | Meaning |
|---|---|
| Annotation | A part that you reject, with your note |
| Generator | The source of the values of a rule |
| Rule | An edit that a medium allows on a part, with its generator |
| Alternate | A variant of the annotated part, or another part |
| Reaction | Your reject or accept of an alternate |
| Invariant | A name and one sentence that states what must be true |

## Machine

1. Start with your annotation as the first rejected alternate.
2. Draw from the `Model` generator: invariants that can explain your note, and one alternate for each invariant that satisfies it.
3. Add the marked text as a literal invariant, the alternates from each rule of the medium, and each other part.
4. In each round, react to the alternate that splits the invariants nearest to half.
5. Keep each invariant that reports each alternate you reject and passes each alternate you accept.
6. Take the invariant with the fewest violations.

Each request to Jev holds one alternate and each invariant as one question under its name. A score of 0.9 or more is a violation.

## weft-hegel format

The generator and the machine use the names of `weft-hegel`, so that the two can merge later.

| invariants | weft-hegel |
|---|---|
| `Generator::Integer(min, max)`, kind `hegel.integer` | `Generator::Integer(min, max)` |
| `Generator::SampledFrom(Vec<Json>)`, kind `hegel.sampled_from` | `Generator::SampledFrom(Vec<Json>)` |
| `Generator::Model(model, context)`, kind `model.alternate` | none |
| `Rule` | a rule of `case.machine(rules, invariants, steps)` |
| invariants | the invariants of `case.machine(rules, invariants, steps)` |
| `Machine::new` | `case.machine` |
| `Machine::round` | `case.round` |
| `Machine::react` with reject | `case.rejected` |
| `Machine::invariant` | `case.complete` with `INTERESTING` |

`Generator::Model` sends the artifact, the part, the marked text, your note and a size to the model. Each value is one alternate with the name and the sentence of the invariant that it satisfies.

## Providers

| Provider | Use |
|---|---|
| `provider/jev.rs` | Scores each invariant on each part |
| `provider/gemini.rs` | Draws the values of `Generator::Model` from `gemini-flash-latest` |

## Mediums

| Medium | Parts | Rules |
|---|---|---|
| Prose | Sentences | `remove`, `keep`, `split` |
| Code | Lines | `rename`, `remove`, `inline`, with ast-grep for edits and rustc for checks |

To add a medium, implement `parts`, `rules` and `run` of the `Medium` trait.

## Commands

| Command | Result |
|---|---|
| `invariants check <file> <invariants.json>` | Each violation, with its place and invariant name |
| `invariants narrow <file>` | Your reactions for each annotation in `<file>.json`, then each invariant and its violations |
| `cargo nextest run -p invariants` | For each example: the wrong violations of the golden invariants, then the flow of each annotation |

## Test

[tests/examples.rs](tests/examples.rs) runs one test for each example. The test reacts for you: it rejects an alternate when the golden invariant of the annotation reports it. The golden invariant of an annotation is the one whose violations hold the annotated part. For each annotation, the flow gives one result:

| Result | Meaning |
|---|---|
| `pass` | The reactions chose an invariant with the golden violations |
| `golden` | The annotated part is in no golden violation |
| `draw` | No drawn invariant has the golden violations; the nearest one follows, with each missed and false part |
| `reactions` | A drawn invariant has the golden violations, but the reactions chose another |

## Examples

- [examples/writing.md](examples/writing.md) and [examples/writing.json](examples/writing.json)
- [examples/code.rs](examples/code.rs) and [examples/code.json](examples/code.json)

Each JSON file holds `annotations`, `invariants` by name, and `violations` by invariant name.

## Setup

Put `JEV_TOKEN=<token>` and `GEMINI_API_KEY=<key>` in `~/.config/invariants/secrets.env`.
