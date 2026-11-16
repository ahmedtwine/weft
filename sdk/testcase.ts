import { AssumeFailed, generate, poolAdd, poolGenerate, target } from "./weft";

type Schema<T> = { _zod: unknown; parse: (value: unknown) => T };
type Json = (schema: Schema<unknown>) => unknown;

export class Generator<T> {
  constructor(readonly doDraw: (tc: TestCase) => T) {}
}

export class TestCase {
  constructor(private readonly json: Json) {}

  draw<T>(generator: Generator<T> | Schema<T>): T {
    return generator instanceof Generator
      ? generator.doDraw(this)
      : generator.parse(generate(this.json(generator)));
  }

  assume(condition: boolean) {
    if (!condition) throw new AssumeFailed();
  }

  target(score: number, label: string) {
    target(score, label);
  }
}

export class Pool<T> {
  constructor(
    _: TestCase,
    private readonly name: string
  ) {}

  add(value: T) {
    poolAdd(this.name, value);
  }

  valuesReusable() {
    return new Generator(() => poolGenerate(this.name, false) as T);
  }

  valuesConsumed() {
    return new Generator(() => poolGenerate(this.name, true) as T);
  }
}
