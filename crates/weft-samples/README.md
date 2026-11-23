# weft-samples

An annotation pairs a rejected span in a file with our note on why it failed. From that note, the model writes candidate invariants and one alternate span per candidate. As we accept or reject each alternate, the list narrows down to the weakest invariant that still fails on the annotated span. `weft-samples check` then checks new files against that invariant.

## Types

| Type | Meaning |
|---|---|
| Annotation | Rejected part of a file and our note on why it failed |
| Generator | Source of values for an edit rule |
| Rule | Edit on a part in a medium, paired with its generator |
| Alternate | Variant of the annotated part, or another part from the same file |
| Reaction | Accept or reject on an alternate |
| Example | Text and a person's comment; an annotation is one example |
| Candidate | Name and measure in the shared format across all invariant sources |
| Invariant | Name and one sentence stating what has to hold |

## Machine

`Machine` narrows the invariants over interactive rounds and calls Jev to score each part on each invariant. One Jev call scores one alternate against one invariant, where 0.82 or higher counts as a violation.

1. Seed the rejected alternates with the initial annotation.
2. Pull candidate invariants and one passing alternate per invariant from the `Model` generator.
3. Put the marked text in as a literal invariant, and add alternates from each medium rule and from the remaining parts of the artifact.
4. Pick the alternate on each round that splits the surviving invariants closest to half, and record our reaction.
5. Keep only the invariants that reject every rejected alternate and pass every accepted alternate.
6. Return whichever surviving invariant has the fewest violations in total.

## weft-hegel format

`Generator` and `Machine` keep the `weft-hegel` names, but the search goes in the opposite direction. Weft holds the invariants fixed and searches for a state; `weft-samples` holds the alternates fixed and searches for an invariant.

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

`Generator::Model` takes the artifact, the part, the marked text, the note, and a sample size. For each invariant it produces the name, the one-sentence rule, and one alternate.

## Model

`src/model` is the only module in the crate that talks to external APIs.

| File | Use |
|---|---|
| `model/jev.rs` | Scores each part on each invariant through Jev |
| `model/gemini.rs` | `gemini-flash-latest` client for annotation alternates and example candidates |
| `model/mod.rs` | `Example`, `Source`, `Given`, the `Model` trait, and `Live` (caches responses on disk and only calls the API on a miss) |

## Candidates

Every invariant source outputs `Candidate { name, measure }`. The `measure` field uses the format that the medium's judge expects.

| Source | Measure | Judge |
|---|---|---|
| Marked text of an annotation | One sentence | Jev |
| `Model::alternates` on a prose or code annotation | One sentence | Jev |
| `Model::candidates` on examples with a grammar in their context | One condition in that grammar | Grammar owner |
| Templates from [weft-invariants](../weft-invariants/README.md) | One weft condition | weft |

## Sources

A `Source` holds the examples and their shared context. `Given` stores both in memory.

| Source | Text of an example | Note | Context |
|---|---|---|---|
| Annotation | Marked text | Annotation note | Artifact and part |
| System tests | One test file | Test file path | State keys, actions, and condition grammar |

## Mediums

A `Medium` breaks an artifact into parts and defines the edit rules for those parts.

| Medium | Parts | Rules |
|---|---|---|
| Prose | Sentences | `remove`, `keep`, `split` |
| Code | Lines | `rename`, `remove`, `inline` (ast-grep runs the edits; rustc checks them) |

A new medium implements `parts`, `rules`, and `run` on `Medium`. A new source implements `examples` and `context` on `Source` and puts the measure grammar in that context.

## Commands

| Command | Result |
|---|---|
| `weft-samples check <file> <invariants.json>` | Prints each violation, its location, and the invariant name |
| `weft-samples narrow <file>` | Collects reactions for each annotation in `<file>.json`, then prints each chosen invariant and its violations |

## To do

| Point | State |
|---|---|
| End-to-end flow test for each example | Disabled for now because the check fails. Our golden invariants give 0 false violations, but the golden violations miss the annotated part itself on 1 of 1 annotations in `examples/code.rs` and 4 of 6 annotations in `examples/writing.md`. Commit `0792b1b` has the last version of `tests/examples.rs`, which used an offline `Golden` mock of `Model`. |

## Examples

- [examples/writing.md](examples/writing.md) and [examples/writing.json](examples/writing.json)
- [examples/code.rs](examples/code.rs) and [examples/code.json](examples/code.json)

The JSON files hold the `annotations` list, the `invariants` map keyed by name, and the `violations` map keyed by invariant name.

## Setup

Write `JEV_TOKEN=<token>` and `GEMINI_API_KEY=<key>` to `~/.config/weft-samples/secrets.env`. `Live` stores cached model responses under `~/.cache/weft-samples`.

## Finding

We wanted to see if we could recover system invariants from an existing test suite. For the benchmark, we picked an application state machine where we had previously replaced 2 unit tests with 7 hand-written weft invariants. We ran three candidate sources through [weft-invariants](../weft-invariants/README.md).

| Source of the candidates | Candidates | True in the weft states | Equal to an invariant of the person |
|---|---|---|---|
| Templates on the states of one weft run with no invariants (130 runs, 69 distinct states) | 23 | 23 | 3 |
| Templates on the 9 states of the 2 unit tests | 12 | 2 | 1 |
| The model, with the 2 unit tests and the keys of the state | 4 | 3 | 0 |

We count two invariants as equal when they rule out the exact same states across the full state space, which we define as the Cartesian product of every value seen for each state key during the run.

- Our 23 candidates from the invariant-free weft run cover every state that the 7 hand-written invariants forbid, plus roughly 900 more states.
- Templates on the 2 unit tests only saw 9 states and overfit to them, leaving 10 of the 12 candidates false on the full weft state set (such as "state booting gives event boot").
- The 1 false candidate out of 4 from the model was only missing the condition that the event does not throw.

### Strategy to explore

Combining the unconstrained state trace with both candidate generators gives us a five-step pipeline to try:

1. We run the weft test without invariants and log the state after each step.
2. We build `Candidate` records from templates on the logged states and from the model on the unit tests.
3. Any candidate that evaluates to false on a logged state gets dropped.
4. From the remaining candidates, we choose the smallest subset that forbids the most unreached states.
5. Candidates that still forbid states beyond our known invariants go to human review, since each one either captures an unwritten invariant or marks a state that the search missed. `Machine` already runs the rounds and reactions for step 5, though step 4 is not wired into it yet.

### Open points

| Point | State |
|---|---|
| Run-to-run variance in the candidate set | Three runs gave 17, 19, and 23 candidates, and only the first 3 appeared in all three runs. We should only keep a candidate if it holds across multiple runs. |
| Fault-injection score (whether faults that break an invariant also break tests) | We tested 24 injected state-machine faults with 30 test cases per fault. Unit tests failed on 15 faults (and existing invariants also failed on 12 of those 15), and the other 9 faults failed only on a generated candidate. At 30 cases per fault, random sampling noise can trip a candidate on its own, so the score still needs a no-fault control run (code not in repo). |
| Second system | We ran the same check on a line-protocol process with a 6-action driver over 6 runs and 47 states. It gave initial candidates "disk equal to text", "blocks equal to changes", and "event not save or edit gives previous equal to text" (driver not in repo). |
| Lists | The templates only read string and integer state keys right now; we do not have a template for `forall` conditions over lists yet. |
