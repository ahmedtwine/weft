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
