# weft

A distributed runner for property tests, built on [Hegel](https://hegel.dev) and [KDL](https://kdl.dev). A diagram names objects, their state, the steps between them and the invariants that hold across them. Hegel draws the steps, runs them on each runner and shrinks a failing case to the fewest steps.

## Run the examples

```sh
mise trust && mise install
mise //examples:test      # build weft and each runner, then run each diagram
mise //examples:report    # serve the report at http://127.0.0.1:7357
```

| Path                                              | Shows                                                        |
| ------------------------------------------------- | ------------------------------------------------------------ |
| `examples/runners/{bun,rust,go,python}`           | One door in each language                                    |
| `examples/tests/test_single_runner`               | One diagram for each door runner                             |
| `examples/apps/{orders,addresses,shop,courier}`   | A shop of four apps in Go, Rust, Python and TypeScript       |
| `examples/tests/test_multiple_runner/parcel.weft.kdl` | One invariant over the four apps                         |

## State

A runner gives its state to weft with one call. The call takes a native value, or a function that reads it, under `"<object>"` for a record of keys or `"<object>.<key>"` for one key:

| Language   | SDK          | Call                                              | Start in `weft.kdl`                          |
| ---------- | ------------ | ------------------------------------------------- | -------------------------------------------- |
| TypeScript | `sdk/`       | `state("door", () => door)` from `"weft"`         | `runner "<name>" harness="bun"`              |
| Rust       | `sdk/rust`   | `weft_sdk::state("door", now)` with `Serialize`   | `runner "<name>" command="<binary>"`         |
| Go         | `sdk/go`     | `weft.State("door", func() Door { return door })` | `runner "<name>" command="<binary>"`         |
| Python     | `sdk/python` | `weft.state("door", lambda: door)`, a dataclass   | `runner "<name>" command="python -m weft"`   |

Weft reads each registered state after `start` and after each step. The diagram draws the state after `start` as the `«initial»` card of each object.

## A door

`examples/tests/test_single_runner/door.bun.weft.kdl`:

```kdl
setup {
  person "user"
  element "door" state="closed" error="" {
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
  module "door" "../../runners/bun/door.ts"
  start "door.reset"
  run "door.open" input="door.openSchema"
  run "door.close" input="door.closeSchema"
  run "door.lock" input="door.lockSchema"
  run "door.unlock" input="door.unlockSchema"
}
```

`examples/runners/bun/door.ts`:

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

An `on` line gives the value of a key after the step. A key of the setup with no term keeps its value from before the step.

## A shop of four apps

`orders` keeps each order in Redis in a testcontainer. `addresses` keeps the address book. `shop` lets a customer choose the address of an order. `courier` ships an order to its address. Each app serves HTTP and names its url with a `serve` line; weft forwards each request between apps and draws it in the color of the app that sends it.

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
✕ tests/test_multiple_runner/parcel.weft.kdl · "a parcel goes only to the address of its own customer" · courier.parcels[0].customer = "ana" ≠ courier.parcels[0].recipient = "ben"
```

The test fails by design: Hegel finds `choose_address(addr-ben)` and then `ship(o-ana)`.

## Runners

`weft.kdl` names one `runner` line for each `impl` block. Weft starts each command with these variables:

| Variable          | Value                                              |
| ----------------- | -------------------------------------------------- |
| `WEFT_DIAGRAM`    | The path of the diagram                            |
| `WEFT_ADAPTER`    | The name of the `impl` block                       |
| `WEFT_TRANSPORT`  | The socket or url of weft                          |
| `WEFT_LIB`        | The path of libweft, the C ABI over Hegel          |
| `WEFT_<NAME>_URL` | The url of the app of `impl "<name>"`, through weft |

An `impl` block has `module`, `serve`, `start` and `run` lines. The `start` lines run in the order of the `impl` blocks.

## License

MIT. merman is MIT OR Apache-2.0. Icons are Lucide, ISC, in `crates/weft-draw/src/vendor`.
