# weft

A distributed runner for property tests, built on [Hegel](https://hegel.dev) and [KDL](https://kdl.dev).


Adding support for runners in multiple languages.

Currently supported:
- Typescript
- Rust

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

## License

MIT. merman is MIT OR Apache-2.0. Icons are Lucide, ISC, in `crates/weft-draw/src/vendor`.
