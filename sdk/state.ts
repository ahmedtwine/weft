type Read = () => unknown;
type Registered = { read: Read; type?: unknown };
type Shape = Record<string, unknown>;

const registry = ((globalThis as { weftStates?: Map<string, Registered> }).weftStates ??= new Map());

export const state = (key: string, read: Read | object, type?: unknown) => {
  registry.set(key, { read: typeof read === "function" ? (read as Read) : () => read, type });
};

export const states = () =>
  Object.fromEntries(
    [...registry].map(([key, { read }]) => [key, JSON.parse(JSON.stringify(read() ?? null))])
  );

const guessed = (value: unknown): Shape => {
  if (typeof value === "string") return { type: "string" };
  if (typeof value === "boolean") return { type: "boolean" };
  if (typeof value === "number") return { type: Number.isInteger(value) ? "integer" : "number" };
  if (Array.isArray(value)) return { type: "array", items: value.length ? guessed(value[0]) : {} };
  if (value && typeof value === "object")
    return {
      type: "object",
      properties: Object.fromEntries(Object.entries(value).map(([k, v]) => [k, guessed(v)])),
    };
  return {};
};

export const schemas = (json: (type: unknown) => unknown) =>
  Object.fromEntries(
    [...registry].map(([key, { read, type }]) => {
      try {
        return [key, type ? json(type) : guessed(JSON.parse(JSON.stringify(read() ?? null)))];
      } catch {
        return [key, {}];
      }
    })
  );
