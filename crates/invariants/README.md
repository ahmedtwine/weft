# invariants

You mark a part of an artifact that you reject. You react to alternates of that part. You get the weakest invariant that agrees with each reaction, and you lint new content with it.

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
2. Take as invariants the golden invariants and the marked text as a literal invariant.
3. Draw alternates from each rule: first the `Model` generator, then each rule of the medium, then each other part.
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

`Generator::Model` sends the artifact, the part, the marked text, your note and a size to the model. Each value is one alternate with its text and the property that it changes.

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
| `invariants test <file>` | The machine with the golden invariants as the reactions, then the wrong violations |
| `mise run examples` | Both tests |

## Examples

- [examples/writing.md](examples/writing.md) and [examples/writing.json](examples/writing.json)
- [examples/code.rs](examples/code.rs) and [examples/code.json](examples/code.json)

Each JSON file holds `annotations`, `invariants` by name, and `violations` by invariant name.

## Setup

Put `JEV_TOKEN=<token>` and `GEMINI_API_KEY=<key>` in `~/.config/invariants/secrets.env`.
