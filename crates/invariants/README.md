# invariants

Turns a few annotations on an artifact into golden rules. A machine checks each rule on new artifacts and proposes a valid fix for each flag.

## Vision

You mark the parts that you dislike, such as a sentence or a name in code, and you write a note about each part. The narrowing turns your notes into the weakest rule that flags each part you reject and passes each part you accept. The rule then checks new artifacts of the same kind and proposes the edit that clears each flag.

## Method

The method follows Wayne's "showcasing a bug": find the weakest property that the bug violates.

1. **Reaction.** You mark a part and write a note.
2. **Moves.** The medium lists the valid edits of the part. For prose the edits remove, keep or split a clause. For code the edits rename, inline or remove; ast-grep makes each edit, and rustc checks that the file still compiles.
3. **Near-misses.** You reject or accept each edited part and each similar part. An accepted part looks like the bad part and keeps its meaning.
4. **Weakest rule.** The narrowing keeps the weakest candidate rule that flags each rejected part and passes each accepted part.
5. **Golden rule.** The rule becomes golden when it gives your answers on the test file of the folder.

## A golden rule

```kdl
golden "comment" {
    cut "line_comment"
    question "Which job does the comment in `text` do for a reader of the code?"
    flag echo="a comment that restates what the next line of code says"
    pass reason="a comment that gives a reason or a fact that the code leaves out"
    fix "remove"
}
```

1. `cut` picks each candidate: a node kind for code, a word for prose.
2. For each candidate, the decision model returns a probability for each category in `flag` and `pass`, in the context of the whole artifact. The score is the sum of the probabilities of the `flag` categories, and the rule flags a score of 0.5 or more.
3. `split` and `then` add a second question for one category, such as a contrast in prose.
4. `fix` names the move that clears the flag.

A fix is valid when:

1. The edited artifact compiles.
2. The edit lowers the count of flagged parts, and the edited part passes.
3. A rename keeps its object: the decision model returns a high probability that the old name and the new name point at the same object.

Of the valid fixes, the fix with the highest probability wins.

## Golden rules

| Folder | Rule | Flags | Fix | Found | False flags |
|---|---|---|---|---|---|
| `writing` | negation | a denial of a claim that only the writer raises, a report of skipped work, a stock denial of praise | | 20 of 20 | 0 |
| `writing` | actor | a thing or an idea as the subject of a verb for a mind or a body | | 5 of 7 | 1 |
| `writing` | vague | a figure of speech, or a noun for a quality where a countable thing could stand | | 4 of 5 | 4 |
| `code` | vague | a name for a role that fits any program | rename | 2 of 2 | 0 |
| `code` | layer | a wrapper that forwards to one call | inline | 1 of 1 | 0 |
| `code` | alias | two names with one meaning for one object | rename | 3 of 3 | 0 |
| `code` | comment | a comment that restates the next line | remove | 1 of 1 | 0 |

The writing test file holds made-up product documents, a status report, and real comments by people and by models from the Slop or Not game. Each slop denial sits beside a denial that a reader needs: a warning, a limit or an error.

## Parts

| Type | Holds |
|---|---|
| `Medium` | `parts` of an artifact, the `actions` of a part, `run` to apply one action to the artifact, and `cut` to pick candidates |
| `Action` | A verb and the `Generator` of its input |
| `Measure` | A pattern, or a `Choice` of categories for the decision model |
| `Rule` | A measure, its threshold, and the rejected and accepted parts |
| `Narrowing` | The state machine of steps 2 to 4 |

## Commands

| Command | Does |
|---|---|
| `mise run examples` | Narrows each annotation of each example and waits for each reaction |
| `invariants golden <dir>` | Grades the golden rules of a folder on its test file and finds a fix for each flag |
| `invariants check <file> <dir>` | Applies the golden rules of a folder to a file |

## Later

Hegel will generate the moves, and its shrinking will return the nearest near-miss.
