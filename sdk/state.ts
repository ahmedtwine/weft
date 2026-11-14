type Read = () => unknown;

const registry = ((globalThis as { weftStates?: Map<string, Read> }).weftStates ??= new Map());

export const state = (key: string, read: Read | object) => {
  registry.set(key, typeof read === "function" ? (read as Read) : () => read);
};

export const states = () =>
  Object.fromEntries(
    [...registry].map(([key, read]) => [key, JSON.parse(JSON.stringify(read() ?? null))])
  );

export const keys = () => [...registry.keys()];
