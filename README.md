# weft

A distributed runner for property tests, built on [Hegel](https://hegel.dev) and [KDL](https://kdl.dev).


Runners:

| Language   | SDK          | Start                                              |
| ---------- | ------------ | -------------------------------------------------- |
| TypeScript | `sdk/`       | `adapter "bun"`, or `runner "<name>" harness="bun"` |
| Rust       | `sdk/rust`   | `runner "<name>" command="<binary>"`               |
| Go         | `sdk/go`     | `runner "<name>" command="<binary>"`               |
| Python     | `sdk/python` | `runner "<name>" command="python -m weft"`         |

## Diagram

A door that can be closed, open or locked. Opening a locked door should fail.

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
      on "unlock" state="closed"
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
```



```sh
bun add github:ahmedtwine/weft#v0.3.0 zod
```

`weft.kdl`:

```kdl
include "**/*.weft.kdl"
theme "light"
style "person" icon="user"
style "element" icon="door-closed"
adapter "bun" cases=100
```

`src/door.ts`:

```ts
import { z } from "zod";

let state = "closed";

export const doorSchema = z.strictObject({
  state: z.string().meta({ weft: { read: "door.state" } }),
});
export const errorSchema = z.string().meta({ weft: { read: "door.error" } });
export const openSchema = z.strictObject({}).meta({ weft: { verb: "open" } });
export const closeSchema = z.strictObject({}).meta({ weft: { verb: "close" } });
export const lockSchema = z.strictObject({}).meta({ weft: { verb: "lock" } });
export const unlockSchema = z.strictObject({}).meta({ weft: { verb: "unlock" } });

export const door = () => ({ state });
export const reset = () => {
  state = "closed";
};
export const open = () => {
  if (state === "locked") throw new Error("the door is locked");
  state = "open";
};
export const close = () => {
  state = "closed";
};
export const lock = () => {
  state = "locked";
};
export const unlock = () => {
  state = "closed";
};
```

Register your runner `invariants/door.weft.kdl`:

```kdl
impl "bun" {
  module "door" "../src/door.ts"
  start "door.reset"
  read "door.door" output="door.doorSchema"
  run "door.open" input="door.openSchema" error="door.errorSchema"
  run "door.close" input="door.closeSchema"
  run "door.lock" input="door.lockSchema"
  run "door.unlock" input="door.unlockSchema"
}
```

```sh
bunx weft            
bunx weft --ui       # browse the diagrams at http://127.0.0.1:7357
```

## Runners and apps

`weft.kdl` names one `runner` line for each `impl` block. Weft starts the command of each line for each diagram, with these variables:

| Variable              | Value                                          |
| --------------------- | ---------------------------------------------- |
| `WEFT_DIAGRAM`        | The path of the diagram                        |
| `WEFT_ADAPTER`        | The name of the `impl` block                   |
| `WEFT_TRANSPORT`      | The socket or url of weft                      |
| `WEFT_LIB`            | The path of libweft                            |
| `WEFT_<NAME>_URL`     | The url of the app of `impl "<name>"`, through weft |

An app that serves HTTP names its url with a `serve` line. Weft forwards each request between apps, records it as a step of the trace, and draws it in the diagram in the color of the app that sends it. The `start` lines run in the order of the `impl` blocks.

```kdl
impl "authz" {
  module "authz" "../../apps/authz/authz/authz.go"
  serve "authz.url"
  start "authz.reset"
  read "authz.authz" output="authz.authzSchema"
}
```

## Examples

`examples/` is a mise monorepo project. `mise //examples:test` builds weft and each runner, then runs each diagram. `mise //examples:report` serves the report.

| Path                                                 | Shows                                                                 |
| ---------------------------------------------------- | --------------------------------------------------------------------- |
| `examples/runners/{bun,rust,go,python}`              | The door of this README in each language                              |
| `examples/tests/test_single_runner`                  | One diagram for each door runner                                      |
| `examples/apps/{authz,vault,admin,tool}`             | Four apps in Go, Rust, Python and TypeScript; authz runs SpiceDB in a testcontainer |
| `examples/tests/test_multiple_runner`                | One invariant over the four apps; weft finds the cross-tenant read    |

The multiple runner test fails by design. The admin of `acme` points the store of `acme` at the credential of `globex`, and the tool reads that credential for an analyst of `acme`:

```kdl
invariant "a credential is read only for its own tenant" {
  implies {
    neq "vault.read_owner" ""
    eq "vault.read_owner" "tool.tenant"
  }
}
```

```text
✕ tests/test_multiple_runner/credential.weft.kdl · "a credential is read only for its own tenant" · vault.read_owner = "globex" ≠ tool.tenant = "acme"
```

## License

MIT. merman is MIT OR Apache-2.0. Icons are Lucide, ISC, in `crates/weft-draw/src/vendor`.
