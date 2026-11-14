import { state } from "weft";

const door = { state: "closed", error: "" };
state("door", () => door);

const set = (to: string, error = "") => Object.assign(door, { state: to, error });

export const reset = () => set("closed");
export const open = () => (door.state === "locked" ? set("locked", "the door is locked") : set("open"));
export const close = () => set("closed");
export const lock = () => set("locked");
export const unlock = () => set("closed");
