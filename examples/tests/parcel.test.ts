import { test } from "bun:test";
import { weft } from "weft/test";

test("parcel", () => weft("tests/parcel.weft.kdl"), 600_000);
