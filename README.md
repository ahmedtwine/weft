# weft

A distributed runner for property tests, built on [Hegel](https://hegel.dev) and [KDL](https://kdl.dev). A test names its objects, the invariants that hold over their state, and the module of each object. Hegel draws every sequence of the functions of those modules with inputs from their native types, checks each invariant after each step, and shrinks a failing case to the fewest steps and the least values, as `quint run --invariant` does for a Quint spec.

## Install

```sh
bun add -d git+ssh://git@github.com/ahmedtwine/weft.git#v0.4.0
GH_TOKEN=<token> bunx weft tests/*.weft.kdl
```

The `weft` command downloads the binary and libweft of its release on the first run. `GH_TOKEN` gives read access to `ahmedtwine/weft`.

## Run the examples

```sh
mise trust && mise install
mise //examples:test      # build weft and the four runners, then run each test
mise //examples:report    # serve the report at http://127.0.0.1:7357
```

`examples/tests/*.weft.kdl` holds each test. `examples/runners/<lang>/` holds one runner for each language, with a folder of modules for each test. Weft starts the runner of each `impl` block, drives it through each case, and stops it at the end of the test.

## A test

`examples/tests/bank.bun.weft.kdl`:

```kdl
setup {
  person "owner"
  api "bank" {
    invariant "the balance never goes below zero" {
      igte "bank.balance" 0
    }
  }
}
impl "bun" {
  module "bank" "../runners/bun/bank/bank.ts"
}
```

| Line                                 | Gives                                                                                  |
| ------------------------------------ | -------------------------------------------------------------------------------------- |
| `<kind> "<object>"`                  | An object; a kind is any word with `style "<kind>" icon="<lucide icon>"` in `weft.kdl` |
| `invariant "<name>" { <condition> }` | A Quint builtin over the state: `eq`, `implies`, `forall`, `exists` and more           |
| `impl "<lang>" { module … }`         | The runner of a language and the module of each of its objects                         |

An `impl` block holds `module` lines only. The module gives the rest:

| Export of the module | Is                                                                                 |
| -------------------- | ---------------------------------------------------------------------------------- |
| `reset`              | The function that sets the state before each case                                  |
| `url`                | The function that gives the url of an HTTP app                                     |
| each other function  | An action; its name is the verb, the type of its input the schema Hegel draws from |

The input of an action is the native type of its parameter: a `JsonSchema` type in Rust, a struct with `minimum` and `maximum` tags in Go, a dataclass with `Literal` or `Annotated[int, range(…)]` in Python, and the zod schema `<function>Schema` in TypeScript. The first person of `setup` calls each action in the diagram.

`examples/runners/bun/bank/bank.ts`:

```ts
import { state } from "weft";
import { z } from "zod";

const bank = { balance: 0 };
state("bank", () => bank);

export const depositSchema = z.object({ amount: z.int().min(1).max(100) });
export const withdrawSchema = depositSchema;

export const reset = () => {
  bank.balance = 0;
};
export const deposit = ({ amount }: z.infer<typeof depositSchema>) => {
  bank.balance += amount;
};
export const withdraw = ({ amount }: z.infer<typeof withdrawSchema>) => {
  if (amount <= bank.balance) bank.balance -= amount;
};
```

Hegel draws `deposit` and `withdraw` in any order with amounts from 1 to 100, and checks the balance after each step. With the `if` gone from `withdraw`, the test fails and Hegel shrinks the case to one step:

```text
✕ tests/bank.bun.weft.kdl · "the balance never goes below zero" · bank.balance = -1 < 0
```

The same bank runs in each language: `runners/rust/bank/bank.rs`, `runners/go/bank/bank.go` and `runners/python/bank/bank.py`.

## State

A runner gives its state to weft with one call. The call takes a native value, or a function that reads it, under `"<object>"` for a record of keys or `"<object>.<key>"` for one key. The SDK sends the JSON Schema of the native type, and weft checks each key and field that an invariant reads against it before the first case:

| Language   | SDK          | Call                                                       | Start in `weft.kdl`                        |
| ---------- | ------------ | ---------------------------------------------------------- | ------------------------------------------ |
| TypeScript | `sdk/`       | `state("courier.parcels", () => parcels, z.array(Parcel))` | `adapter "bun"`                            |
| Rust       | `sdk/rust`   | `weft_sdk::state("bank", now)` with `JsonSchema`           | `runner "rust" command="<binary>"`         |
| Go         | `sdk/go`     | `weft.State("bank", func() Bank { return bank })`          | `runner "go" command="<binary>"`           |
| Python     | `sdk/python` | `weft.state("bank", lambda: bank)` with a dataclass        | `runner "python" command="python -m weft"` |

The Diagram tab of the report draws the state after `reset` as a table of type, key and value under each object, each step with the keys it changes, and the failing condition. The Run tab holds each step of the case and the state after it.

## A shop of four apps

`tests/parcel.weft.kdl` runs four apps, one in each runner: `orders` in Go keeps each order in Redis in a testcontainer, `addresses` in Rust keeps the address book, `shop` in Python lets a customer choose the address of an order, and `courier` in TypeScript ships an order to its address. An app reaches another app at `WEFT_<MODULE>_URL`; weft forwards each request and draws it in the color of the app that sends it.

The orders app checks that the customer owns the order, and the address book trusts the courier, so a customer can choose the address of another customer:

```kdl
api "courier" {
  invariant "a parcel goes only to the address of its own customer" {
    forall "p" in="courier.parcels" {
      eq "p.customer" "p.recipient"
    }
  }
}
```

```text
✕ tests/parcel.weft.kdl · "a parcel goes only to the address of its own customer" · courier.parcels[0].customer = "ana" ≠ courier.parcels[0].recipient = "ben"
```

The test fails by design: Hegel finds `choose_address(addr-ben)` and then `ship(o-ana)`.

## Runners

`weft.kdl` names one `runner` line for each language. Weft starts each command with these variables:

| Variable            | Value                                                            |
| ------------------- | ---------------------------------------------------------------- |
| `WEFT_DIAGRAM`      | The path of the test                                             |
| `WEFT_ADAPTER`      | The name of the `impl` block                                     |
| `WEFT_TRANSPORT`    | The socket or url of weft                                        |
| `WEFT_LIB`          | The path of libweft, the C ABI over Hegel                        |
| `WEFT_<MODULE>_URL` | The url of the app of a module with a `url` export, through weft |

The `reset` functions run in the order of the `impl` blocks.

## Crates

| Crate            | Does                                                                                         |
| ---------------- | -------------------------------------------------------------------------------------------- |
| `weft-parse`     | Reads `weft.kdl` and each test into objects, invariants and modules, and evaluates invariants |
| `weft-hegel`     | Drives Hegel through its C ABI: cases, the state machine, generators and shrinking            |
| `weft-simulator` | Reads the hello of each runner, turns its schemas into generators, and runs each case        |
| `weft-runner`    | Starts the runners, carries their messages, and forwards the HTTP calls between apps          |
| `weft-wire`      | The framing of each message between weft and a runner                                        |
| `weft-draw`      | Draws a test and its failing case as a sequence diagram                                      |
| `weft-c`         | libweft, the C ABI that each SDK loads                                                       |
| `weft-cli`       | The `weft` command and its report                                                            |

## License

MIT. `mise run notices` writes `THIRD-PARTY-NOTICES.md`, the license of each part weft ships: Hegel and each crate it links, the Lucide icons, and the scripts and fonts of the report.
