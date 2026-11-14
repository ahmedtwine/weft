# weft

A distributed runner for property tests, built on [Hegel](https://hegel.dev) and [KDL](https://kdl.dev). A test names its objects, the invariants that hold over their state, and the module of each object. Hegel draws every sequence of the functions of those modules with inputs from their native types, checks each invariant after each step, and shrinks a failing case to the fewest steps and the least values, as `quint run --invariant` does for a Quint spec.

## Run the examples

```sh
mise trust && mise install
mise //examples:test      # build weft and the four runners, then run each test
mise //examples:report    # serve the report at http://127.0.0.1:7357
```

| Path                              | Holds                                                         |
| --------------------------------- | ------------------------------------------------------------- |
| `examples/runners/<lang>/`        | One runner for each language: bun, rust, go and python        |
| `examples/runners/<lang>/<test>/` | The modules of one test in that language                      |
| `examples/tests/*.weft.kdl`       | Each test; its `impl "<lang>"` blocks name the modules to run |

Weft starts the runner of each `impl` block, drives it through each case, and stops it when the test ends:

```text
▸ tests/parcel.weft.kdl · go · runners/go/bin/runner · pid 19877
▸ tests/parcel.weft.kdl · rust · runners/rust/target/release/runner · pid 19887
▸ tests/parcel.weft.kdl · python · uv run --quiet --project runners/python python -m weft · pid 19897
▸ tests/parcel.weft.kdl · bun · bun test · pid 19898
■ tests/parcel.weft.kdl · rust · exit status: 0 · 1.30s
■ tests/parcel.weft.kdl · go · exit status: 0 · 1.52s
```

## A test

`examples/tests/door.bun.weft.kdl`:

```kdl
setup {
  person "user"
  device "door" {
    invariant "an error only when locked" {
      implies {
        neq "door.error" ""
        eq "door.state" "locked"
      }
    }
  }
}
impl "bun" {
  module "door" "../runners/bun/door/door.ts"
}
```

| Line                                 | Gives                                                                                  |
| ------------------------------------ | -------------------------------------------------------------------------------------- |
| `<kind> "<object>"`                  | An object; a kind is any word with `style "<kind>" icon="<lucide icon>"` in `weft.kdl` |
| `invariant "<name>" { <condition> }` | A Quint builtin over the state: `eq`, `implies`, `forall`, `exists` and more           |
| `impl "<lang>" { module … }`         | The runner of a language and the module of each of its objects                         |

An `impl` block holds `module` lines only. The module gives the rest:

| Export of the module | Is                                                                     |
| -------------------- | ---------------------------------------------------------------------- |
| `reset`              | The function that sets the state before each case                      |
| `url`                | The function that gives the url of an HTTP app                         |
| each other function  | An action; its name is the verb, the type of its input the schema Hegel draws from |

The input of an action is the native type of its parameter: a `JsonSchema` type in Rust, a struct in Go, a dataclass or `Literal` in Python, and the zod schema `<function>Schema` in TypeScript. The first person of `setup` calls each action in the diagram.

`examples/runners/bun/door/door.ts`:

```ts
import { state } from "weft";

const door = { state: "closed", error: "" };
state("door", () => door);

const set = (to: string, error = "") => Object.assign(door, { state: to, error });

export const reset = () => set("closed");
export const open = () => (door.state === "locked" ? set("locked", "the door is locked") : set("open"));
export const close = () => set("closed");
export const lock = () => set("locked");
export const unlock = () => set("closed");
```

## State

A runner gives its state to weft with one call. The call takes a native value, or a function that reads it, under `"<object>"` for a record of keys or `"<object>.<key>"` for one key. The SDK sends the JSON Schema of the native type, and weft checks each key and field that an invariant reads against it before the first case:

| Language   | SDK          | Call                                                       | Start in `weft.kdl`                        |
| ---------- | ------------ | ---------------------------------------------------------- | ------------------------------------------ |
| TypeScript | `sdk/`       | `state("courier.parcels", () => parcels, z.array(Parcel))` | `adapter "bun"`                            |
| Rust       | `sdk/rust`   | `weft_sdk::state("door", now)` with `JsonSchema`           | `runner "rust" command="<binary>"`         |
| Go         | `sdk/go`     | `weft.State("door", func() Door { return door })`          | `runner "go" command="<binary>"`           |
| Python     | `sdk/python` | `weft.state("door", lambda: door)` with a dataclass        | `runner "python" command="python -m weft"` |

The Diagram tab draws the state after `start` as a table of type, key and value under each object, each step with the keys it changes, and the failing condition. The Run tab holds each step of the case and the state after it.

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

The test fails by design: Hegel finds `choose_address(addr-ben)` and then `ship(o-ana)`. Its `impl` blocks hold one `module` line each:

```kdl
impl "go" {
  module "orders" "../runners/go/parcel/orders.go"
}
impl "rust" {
  module "addresses" "../runners/rust/parcel/addresses.rs"
}
impl "python" {
  module "shop" "../runners/python/parcel/shop.py"
}
impl "bun" {
  module "courier" "../runners/bun/parcel/courier.ts"
}
```

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

## License

MIT. merman is MIT OR Apache-2.0. Icons are Lucide, ISC, in `crates/weft-draw/src/vendor`.
