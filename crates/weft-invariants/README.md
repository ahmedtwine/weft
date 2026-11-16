# weft-invariants

Experimental. This crate is a small base for later work; see the finding and the strategy in [weft-samples](../weft-samples/README.md#finding).

It takes a weft test and candidates in the format of `weft-samples`. It gives the weft invariants that no state of the test makes false.

## Flow

1. Run the test with no invariants and record the state after each step.
2. Make the space: each mix of the values that the run gave for each key. A key is in the space when each value is a text or a whole number and it has 32 values or fewer.
3. Get the candidates. The templates give `implies` conditions with two conditions or fewer (`eq`, `neq`, `in`) and one result (`eq` with a value or with another key). `examples=<file>` adds the candidates of the model for that test file.
4. weft parses each candidate and judges it on each state of the space.
5. Keep the fewest template candidates that forbid the most states that the run did not reach.
6. Print each kept candidate that is true in each recorded state as a weft `invariant`.

## Command

```sh
weft-invariants <directory> <test> [cases=<count>] [steps=<count>] [examples=<file>]…
```

`<directory>` has the `weft.kdl` of the test. The runners of the test must be built.

| Line of the output | Meaning |
|---|---|
| `states` | The runs, the states, the distinct states and the size of the space |
| `search` | The count of kept template candidates and the states that they forbid |
| `<count>  true in the search  <source>  "<name>"` | One candidate, the count of states that it forbids, and its source: `search`, `model` or `golden` |
| `= golden "<name>"` | The candidate forbids the same states as an invariant of the test |
| `not inside the search set` | An invariant of the test forbids a state that the kept candidates allow |
| `invariants` | The weft text of each candidate that is true in the search |

## Test

`cargo nextest run -p weft-invariants` runs the search on the states of a door, with no runner and no model.

## Limits

| Limit | Value |
|---|---|
| Size of the space | 1,048,576 states |
| Values of one key | 32 |
| Lists and `forall` | Not read |
| Model | `examples=<file>` needs the keys of `weft-samples` |
