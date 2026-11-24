# weft-world

`weft-world` is the home for a language and a type checker for worlds. A world is a formal model of a domain that is hard to formalize, such as writing. The crate has no code yet.

## Vision

You mark a span of text and name what is wrong with it. Each mark must become a rule that fits the grammar of the world. When the grammar cannot separate your marked span from a span you accepted, the world refuses the mark, shows you the two spans side by side, and asks which difference decides the verdict. Your answer adds one named distinction to the grammar. Each mark makes the world more precise, and you never have to state the full rule in advance.

## Layers

| Layer | Contents |
|---|---|
| Grammar | The types of nodes, roles and relations that a text can have |
| World rules | The shapes that a rule can take over the grammar, and the limits on rule size |
| Rules | A pattern on the graph, a fault name, and a fix that rewrites the graph |
| Marks | Your comments, each attached to a node or an edge of the graph |

## Grammar

For prose, the grammar starts from two published annotation schemes. Both cover any English text and come with a manual and a hand-annotated corpus.

- AMR gives each verb an event node with numbered roles. `:ARG0` is the doer, `:ARG1` is the thing acted on, and `:polarity -` marks a negation.
- PDTB-3 names the relation between two clauses. `Expansion.Substitution` holds when the text presents two exclusive alternatives and rules one out.

The sentence "Ship production-grade AI, not demos" gives this graph:

| Node | Event | Doer | Object | Polarity |
|---|---|---|---|---|
| c1 | ship | you | production-grade AI | + |
| c2 | ship | you | demos | − |

The edge `c1 → c2` has the relation `Substitution.Arg1-as-subst`. A mark on "not demos" attaches to `c2` and to that edge. The rule for the mark is the pattern "a Substitution edge whose ruled-out node has polarity −", the fault "ruled-out alternative", and the fix "delete the ruled-out node".

## Loop

1. You mark a span.
2. The parser builds the graph of the span.
3. The learner searches the world rules for the smallest pattern that matches the marked span and no accepted span.
4. If it finds a pattern, the world adds the rule.
5. If two spans have the same graph at every node a pattern can see, the world refuses the mark and asks you for the difference.
6. Your answer becomes a new type or attribute in the grammar, and the learner searches again.

## Research

| Work | What we take from it |
|---|---|
| [PDTB-3 Annotation Manual](https://catalog.ldc.upenn.edu/docs/LDC2019T05/PDTB3-Annotation-Manual.pdf), Webber et al. 2019 | Relations between clauses, including `Expansion.Substitution` |
| [AMR guidelines](https://github.com/amrisi/amr-guidelines/blob/master/amr.md) and [Banarescu et al. 2013](https://aclanthology.org/W13-2322/) | Event nodes, doer roles, and negation as `:polarity` |
| [Weiss, Goldberg, Yahav 2017](https://arxiv.org/abs/1711.09576) | The L\* loop of membership queries and counterexamples |
| [Patsantzis, Muggleton 2021](https://arxiv.org/abs/2106.07464) | Metarules that decide which rules can exist, learned from a size limit |
| [Cropper, Morel 2020](https://arxiv.org/abs/2005.02259) | Each failed rule adds a constraint that prunes the space of rules |
| [Yu et al. 2026](https://arxiv.org/abs/2607.01585) | A language model invents new predicates, and Prolog checks each one |
| [Lam et al. 2024](https://arxiv.org/abs/2404.12259) | A language model proposes concepts from raw text, each with inclusion criteria |
