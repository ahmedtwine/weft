# weft-samples

You mark a part of any artifact with a note. A model writes the invariants that can explain your note, with one alternate of the part for each invariant. Your reactions to the alternates leave the weakest invariant that the part violates, and you lint new content with it.

## Types

| Type | Meaning |
|---|---|
| Annotation | A part that you reject, with your note |
| Generator | The source of the values of a rule |
| Rule | An edit that a medium allows on a part, with its generator |
| Alternate | A variant of the annotated part, or another part |
| Reaction | Your reject or accept of an alternate |
| Example | A text and what a person says about it; an annotation is one example |
| Candidate | A name and one measure; each source of invariants gives this format |
| Invariant | A name and one sentence that states what must be true |

## Machine

1. Start with your annotation as the first rejected alternate.
2. Draw from the `Model` generator: invariants that can explain your note, and one alternate for each invariant that satisfies it.
3. Add the marked text as a literal invariant, the alternates from each rule of the medium, and each other part.
4. In each round, react to the alternate that splits the invariants nearest to half.
5. Keep each invariant that reports each alternate you reject and passes each alternate you accept.
6. Take the invariant with the fewest violations.

Each request to Jev holds one alternate and one invariant. A score of 0.82 or more is a violation.

## weft-hegel format

The generator and the machine use the names of `weft-hegel`, but the two machines stay apart. Weft keeps the invariants fixed and looks for a state. The machine of this crate keeps the alternates fixed and looks for an invariant.

| weft-samples | weft-hegel |
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

## Model

All model code is in `src/model`. No other module sends a request.

| File | Use |
|---|---|
| `model/jev.rs` | Scores each invariant on each part |
| `model/gemini.rs` | Draws from `gemini-flash-latest`: alternates for an annotation, and candidates for examples |
| `model/mod.rs` | `Example`, `Source`, `Given`, the `Model` trait, and `Live`, the model that answers from the cache and sends a request only for a missing answer |

## Candidates

Each source of invariants gives the same format, `Candidate { name, measure }`. The measure has the form of its medium.

| Source | Measure | Judge |
|---|---|---|
| The marked text of an annotation | One sentence | Jev |
| `Model::alternates`, for an annotation on prose or code | One sentence | Jev |
| `Model::candidates`, for examples with a grammar in their context | One condition in that grammar | The owner of the grammar |
| The templates of [weft-invariants](../weft-invariants/README.md) | One weft condition | weft |

## Sources

A `Source` gives examples and a context. `Given` is the plain source.

| Source | Text of an example | Note | Context |
|---|---|---|---|
| An annotation | The marked text | Your note | The artifact and the part |
| The tests of a system | One test file | Its path | The keys of the state, the actions and the grammar of a condition |

## Mediums

| Medium | Parts | Rules |
|---|---|---|
| Prose | Sentences | `remove`, `keep`, `split` |
| Code | Lines | `rename`, `remove`, `inline`, with ast-grep for edits and rustc for checks |

To add a medium, implement `parts`, `rules` and `run` of the `Medium` trait. To add a source, implement `examples` and `context` of the `Source` trait, and put the grammar of its measure in the context.

## Commands

| Command | Result |
|---|---|
| `weft-samples check <file> <invariants.json>` | Each violation, with its place and invariant name |
| `weft-samples narrow <file>` | Your reactions for each annotation in `<file>.json`, then each invariant and its violations |

## To do

| Point | State |
|---|---|
| The flow test of each example | It is removed until it passes. The golden invariants give 0 wrong violations, but the annotated part of 1 of 1 annotations of `examples/code.rs` and of 4 of 6 annotations of `examples/writing.md` is in no golden violation, so the flow fails for them. The last version is `tests/examples.rs` of commit `0792b1b`. It used `Golden`, a mock of the `Model` trait, and sent no model request. |

## Examples

- [examples/writing.md](examples/writing.md) and [examples/writing.json](examples/writing.json)
- [examples/code.rs](examples/code.rs) and [examples/code.json](examples/code.json)

Each JSON file holds `annotations`, `invariants` by name, and `violations` by invariant name.

## Setup

Put `JEV_TOKEN=<token>` and `GEMINI_API_KEY=<key>` in `~/.config/weft-samples/secrets.env`. The cache of each answer is in `~/.cache/weft-samples`.

## Finding

The question: can the invariants of a system come from its tests? The measure used one state machine of an app. It has 7 weft invariants that a person wrote, and they replaced 2 unit tests. [weft-invariants](../weft-invariants/README.md) ran each source.

| Source of the candidates | Candidates | True in the weft states | Equal to an invariant of the person |
|---|---|---|---|
| Templates on the states of one weft run with no invariants (130 runs, 69 distinct states) | 23 | 23 | 3 |
| Templates on the 9 states of the 2 unit tests | 12 | 2 | 1 |
| The model, with the 2 unit tests and the keys of the state | 4 | 3 | 0 |

Two invariants are equal when they forbid the same states. The states are the space: each mix of the values that the run gave for each key.

- The 23 candidates of the search forbid each state that the 7 invariants forbid, and approximately 900 states more.
- The unit tests alone overfit: the weft states made 10 of 12 candidates false, for example "state booting gives event boot".
- The model alone missed a condition: its false candidate did not say that the event does not throw.

### Strategy to explore

1. Run the weft test with no invariants and record the state after each step.
2. Get candidates from each source in the one format: the templates on the states, and the model with the tests.
3. Remove each candidate that a state makes false.
4. Keep the fewest candidates that forbid the most states that the run did not reach.
5. Show a person each candidate that forbids more than the known invariants. It is a hidden invariant, or the search did not reach a state. The `Machine` of this crate has the rounds and the reactions for this step; it is not joined to step 4.

### Open points

| Point | State |
|---|---|
| The set changes from run to run | Three runs gave 17, 19 and 23 candidates. Only the first 3 stayed the same. Keep a candidate only when it is true in more than one run. |
| A score from faults: an invariant is important when a fault that breaks it also fails many tests | One measure: 24 faults in the state machine, 30 test cases for each. 15 faults failed a unit test, and an invariant failed for 12 of them. For 9 other faults, only a candidate failed. The measure is not reliable: with 30 test cases a candidate can fail from a different sample, so it needs a control run with no fault. Its code is not in the repo. |
| A second system | A process with a line protocol, a driver with 6 actions, 6 runs and 47 states. The first candidates were "disk equal to text", "blocks equal to changes" and "event not save or edit gives previous equal to text". Its driver is not in the repo. |
| Lists | The templates read only the keys with a text or a whole number. A condition with `forall` has no template. |
