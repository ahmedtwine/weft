import type { TestCase } from "@hegeldev/hegel";
import * as gs from "@hegeldev/hegel/generators";

type Fields = Record<string, unknown>;
type Def = Fields & { type: string; checks?: { _zod: { def: Fields } }[] };
export type Schema = {
  _zod: { def: Def };
  parse: (value: unknown) => unknown;
  safeParse: (value: unknown) => { success: boolean; data?: unknown };
};
type Drawn = gs.PrintableGenerator<unknown>;
type Bound = { at?: number; past?: number };

const ASCII = gs.characters({ maxCodepoint: 127 });
const WHOLE = ["safeint", "int32", "uint32"];
const FORMATS: Record<string, () => Drawn> = {
  email: gs.emails,
  url: gs.urls,
  uuid: () => gs.uuids(),
  date: gs.dates,
  time: gs.times,
  datetime: gs.datetimes,
};

const checks = (def: Def): Fields[] => [def, ...(def.checks ?? []).map((check) => check._zod.def)];
const found = <T>(def: Def, check: string, key: string) =>
  checks(def).find((each) => each.check === check)?.[key] as T | undefined;
const sized = (def: Def) => {
  const length = found<number>(def, "length_equals", "length");
  return {
    minSize: length ?? found<number>(def, "min_length", "minimum"),
    maxSize: length ?? found<number>(def, "max_length", "maximum"),
  };
};
const bound = (def: Def, check: string): Bound => {
  const { value, inclusive } = checks(def).find((each) => each.check === check) ?? {};
  if (typeof value !== "number") return {};
  return inclusive ? { at: value } : { past: value };
};

const string = (def: Def): Drawn => {
  const pattern = found<RegExp>(def, "string_format", "pattern");
  if (pattern) return gs.fromRegex(pattern.source, { fullmatch: true, alphabet: ASCII });
  const format = (def.format ?? found<string>(def, "string_format", "format")) as string | undefined;
  if (!format) return gs.text(sized(def));
  const made = FORMATS[format];
  if (!made) throw new Error(`weft draws no string of the format ${format}`);
  return made();
};

const number = (def: Def): Drawn => {
  const min = bound(def, "greater_than");
  const max = bound(def, "less_than");
  return checks(def).some((each) => WHOLE.includes(each.format as string))
    ? gs.integers({
        minValue: min.at ?? (min.past === undefined ? Number.MIN_SAFE_INTEGER : min.past + 1),
        maxValue: max.at ?? (max.past === undefined ? Number.MAX_SAFE_INTEGER : max.past - 1),
      })
    : gs.floats({
        minValue: min.at,
        minValueExclusive: min.past,
        maxValue: max.at,
        maxValueExclusive: max.past,
        allowNan: false,
        allowInfinity: false,
      });
};

const json: Drawn = gs.recursive(
  gs.oneOf<unknown>(gs.none(), gs.booleans(), gs.integers(), gs.text()),
  (children) =>
    gs.oneOf<unknown>(
      gs.arrays(children, { maxSize: 3 }),
      gs.maps(gs.text(), children, { maxSize: 3 }).map((all) => Object.fromEntries(all))
    )
);

export const accepted = (schema: Schema, value: unknown): [unknown] | [] => {
  try {
    const result = schema.safeParse(value);
    return result.success ? [result.data] : [];
  } catch {
    return [];
  }
};

export const generators = (given: (schema: Schema) => Drawn | undefined) => {
  const known = new Map<Schema, Drawn>();
  const open = new Set<Schema>();
  const all = (schemas: unknown) => (schemas as Schema[]).map(of);
  const inner = (def: Def) => of(def.innerType as Schema);

  const object = (def: Def): Drawn => {
    const fields = Object.entries(def.shape as Record<string, Schema>).map(
      ([key, schema]) => [key, schema._zod.def.type === "optional", of(schema)] as const
    );
    return gs.composite((tc: TestCase) =>
      Object.fromEntries(
        fields.flatMap(([key, optional, drawn]) =>
          optional && !tc.draw(gs.booleans()) ? [] : [[key, tc.draw(drawn)]]
        )
      )
    );
  };

  const record = (def: Def): Drawn => {
    const keys = def.keyType as Schema;
    const values = of(def.valueType as Schema);
    return keys._zod.def.type === "enum"
      ? gs.record(
          Object.fromEntries(
            Object.values(keys._zod.def.entries as Fields).map((key) => [key, values])
          )
        )
      : gs.maps(of(keys), values, { maxSize: 4 }).map((entries) => Object.fromEntries(entries));
  };

  const made: Record<string, (def: Def) => Drawn> = {
    string,
    number,
    object,
    record,
    boolean: gs.booleans,
    null: gs.none,
    undefined: () => gs.just(undefined),
    void: () => gs.just(undefined),
    unknown: () => json,
    any: () => json,
    literal: (def) => gs.sampledFrom(def.values as unknown[]),
    enum: (def) => gs.sampledFrom(Object.values(def.entries as Fields)),
    array: (def) => gs.arrays(of(def.element as Schema), sized(def)),
    tuple: (def) => gs.tuples(...all(def.items)),
    union: (def) => gs.oneOf(...all(def.options)),
    intersection: (def) =>
      gs
        .tuples(of(def.left as Schema), of(def.right as Schema))
        .map(([left, right]) => ({ ...(left as Fields), ...(right as Fields) })),
    nullable: (def) => gs.oneOf(gs.none(), inner(def)),
    optional: (def) => gs.oneOf(gs.just(undefined), inner(def)),
    readonly: inner,
    default: inner,
    prefault: inner,
    catch: inner,
    nonoptional: inner,
    lazy: (def) => of((def.getter as () => Schema)()),
    pipe: (def) => of(def.in as Schema),
  };

  const drawn = (schema: Schema): Drawn => {
    const def = schema._zod.def;
    const make = made[def.type];
    if (!make) throw new Error(`weft draws no value of the Zod type ${def.type}`);
    const value = make(def);
    return checks(def).some((each) => each.check === "custom")
      ? value.filter((each) => accepted(schema, each).length > 0)
      : value;
  };

  const of = (schema: Schema): Drawn => {
    const ready = given(schema) ?? known.get(schema);
    if (ready) return ready;
    if (open.has(schema)) return gs.deferred(() => of(schema));
    open.add(schema);
    const value = drawn(schema);
    open.delete(schema);
    known.set(schema, value);
    return value;
  };

  return of;
};

export const generator = generators(() => undefined);
