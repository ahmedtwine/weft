# weft

A distributed runner for property tests, built on [Hegel](https://hegel.dev) and [KDL](https://kdl.dev). A diagram names objects, their state, the steps between them and the invariants that hold across them. Hegel draws the steps, runs them on each runner and shrinks a failing case to the fewest steps.

## Run the examples

```sh
mise trust && mise install
mise //examples:test      # build weft and each runner, then run each diagram
mise //examples:report    # serve the report at http://127.0.0.1:7357
```

| Path                                   | Holds                                                          |
| -------------------------------------- | -------------------------------------------------------------- |
| `examples/runners/<lang>/`             | One runner for each language: bun, rust, go and python         |
| `examples/runners/<lang>/<test>/`      | The modules of one test in that language                       |
| `examples/tests/*.weft.kdl`            | Each test; its `impl "<lang>"` blocks name the modules to run  |

`mise //examples:test` builds the four runners and runs each test. Weft starts the runner of each `impl` block, drives it through each case, and stops it when the test ends:

```text
▸ tests/parcel.weft.kdl · go · runners/go/bin/runner · pid 19877
▸ tests/parcel.weft.kdl · rust · runners/rust/target/release/runner · pid 19887
▸ tests/parcel.weft.kdl · python · uv run --quiet --project runners/python python -m weft · pid 19897
▸ tests/parcel.weft.kdl · bun · bun test · pid 19898
■ tests/parcel.weft.kdl · rust · exit status: 0 · 1.30s
■ tests/parcel.weft.kdl · go · exit status: 0 · 1.52s
```

## State

A runner gives its state to weft with one call. The call takes a native value, or a function that reads it, under `"<object>"` for a record of keys or `"<object>.<key>"` for one key. The SDK sends the JSON Schema of the native type, and weft checks each key and field that an invariant reads against it before the first case:

| Language   | SDK          | Call                                              | Start in `weft.kdl`                          |
| ---------- | ------------ | ------------------------------------------------- | -------------------------------------------- |
| TypeScript | `sdk/`       | `state("courier.parcels", () => parcels, z.array(Parcel))`, the zod type optional | `adapter "bun"`                     |
| Rust       | `sdk/rust`   | `weft_sdk::state("door", now)`, `Serialize` and `JsonSchema`         | `runner "rust" command="<binary>"`           |
| Go         | `sdk/go`     | `weft.State("door", func() Door { return door })`                    | `runner "go" command="<binary>"`             |
| Python     | `sdk/python` | `weft.state("door", lambda: door)`, a dataclass                      | `runner "python" command="python -m weft"`   |

Weft reads each registered state after `start` and after each step. The diagram draws the state after `start` as a table of type, key and value under each object, and the keys each step changes under the object of that step.

## A door

`examples/tests/door.bun.weft.kdl`:

```kdl
setup {
  person "user"
  device "door" state="closed" error="" {
    state state="closed" {
      on "open" state="open"
      on "lock" state="locked"
    }
    state state="open" {
      on "close" state="closed"
    }
    state state="locked" {
      on "open" error="the door is locked"
      on "unlock" state="closed" error=""
    }
    invariant "an error only when locked" {
      implies {
        neq "door.error" ""
        eq "door.state" "locked"
      }
    }
  }
}
action {
  open from="user" to="door"
  close from="user" to="door"
  lock from="user" to="door"
  open from="user" to="door"
  unlock from="user" to="door"
}
impl "bun" {
  module "door" "../runners/bun/door/door.ts"
  start "door.reset"
  run "door.open" input="door.openSchema"
  run "door.close" input="door.closeSchema"
  run "door.lock" input="door.lockSchema"
  run "door.unlock" input="door.unlockSchema"
}
```

`examples/runners/bun/door/door.ts`:

```ts
import { state } from "weft";
import { z } from "zod";

const door = { state: "closed", error: "" };
state("door", () => door);

export const openSchema = z.strictObject({}).meta({ weft: { verb: "open" } });
export const closeSchema = z.strictObject({}).meta({ weft: { verb: "close" } });
export const lockSchema = z.strictObject({}).meta({ weft: { verb: "lock" } });
export const unlockSchema = z.strictObject({}).meta({ weft: { verb: "unlock" } });

const set = (to: string, error = "") => Object.assign(door, { state: to, error });

export const reset = () => set("closed");
export const open = () => (door.state === "locked" ? set("locked", "the door is locked") : set("open"));
export const close = () => set("closed");
export const lock = () => set("locked");
export const unlock = () => set("closed");
```

A kind such as `device` is any word with a `style "<kind>" icon="<lucide icon>"` line in `weft.kdl`. An `on` line gives the value of a key after the step. A key of the setup with no term keeps its value from before the step.

## A shop of four apps

`tests/parcel.weft.kdl` runs four apps, one in each runner: `orders` in Go keeps each order in Redis in a testcontainer, `addresses` in Rust keeps the address book, `shop` in Python lets a customer choose the address of an order, and `courier` in TypeScript ships an order to its address. An app serves HTTP and names its url with a `serve` line; it reaches another app at `WEFT_<APP>_URL`, and weft forwards each request and draws it in the color of the app that sends it.

The orders app checks that the customer owns the order, and the address book trusts the courier, so the customer can choose an address of another customer:

```kdl
api "courier" {
  on "ship"
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

`weft.kdl` names one `runner` line for each language. An `impl "<lang>"` block of a test starts that runner. Weft starts each command with these variables:

| Variable          | Value                                              |
| ----------------- | -------------------------------------------------- |
| `WEFT_DIAGRAM`    | The path of the diagram                            |
| `WEFT_ADAPTER`    | The name of the `impl` block                       |
| `WEFT_TRANSPORT`  | The socket or url of weft                          |
| `WEFT_LIB`        | The path of libweft, the C ABI over Hegel          |
| `WEFT_<APP>_URL`  | The url of the app of a `serve "<app>.<function>"` line, through weft |

An `impl` block has `module`, `serve`, `start` and `run` lines. The `start` lines run in the order of the `impl` blocks.

## License

MIT. merman is MIT OR Apache-2.0. Icons are Lucide, ISC, in `crates/weft-draw/src/vendor`.
