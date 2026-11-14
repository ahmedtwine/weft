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
