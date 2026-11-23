# weft-invariants

`weft-invariants` is an experiment. We built it as a base for future work after writing up our findings and strategy in [weft-samples](../weft-samples/README.md#finding). Give it a weft test and candidate invariants in the `weft-samples` format, and it prints the weft invariants that hold in every state recorded during the test.

## Flow

First we run the test with no invariants and record the state after each step. The state space comes from every combination of values seen on each key during the run, provided all of that key's values are strings or integers. Keys with over 32 values are dropped.

Next we load the candidate invariants. Templates give us `implies` rules with two conditions or fewer (`eq`, `neq`, `in`) and one `eq` result against a value or another key; passing `examples=<file>` also pulls in the model candidates for that test file. `weft` parses all of them and evaluates each candidate on every state in the space.

From the template candidates, we keep the smallest set that rules out the most states never reached during the run. Every kept candidate that held across all recorded states prints as a weft `invariant`.

## Command

```sh
weft-invariants <directory> <test> [cases=<count>] [steps=<count>] [examples=<file>]…
```

Pass the path to the folder holding `weft.kdl` as `<directory>`. Build the test runners before you run this command.

| Line of the output | Meaning |
|---|---|
| `states` | Run count, recorded states, distinct states, and total size of the state space |
| `search` | How many template candidates we kept and how many states they rule out |
| `<count>  true in the search  <source>  "<name>"` | One candidate, the count of states it rules out, and where it came from (`search`, `model`, or `golden`) |
| `= golden "<name>"` | This candidate rules out the exact same states as an invariant already written in the test |
| `not inside the search set` | An invariant in the test rules out a state that the kept candidates still allow |
| `invariants` | The weft definition for every candidate that holds across the search |

## Test

Run `cargo nextest run -p weft-invariants` to test the search over a door state space with no runner or model.

## Limits

| Limit | Value |
|---|---|
| Size of the space | 1,048,576 states |
| Values of one key | 32 |
| Lists and `forall` | Not read |
| Model | `examples=<file>` requires the keys used by `weft-samples` |
