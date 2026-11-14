export type WeftMeta = {
  maxLeaves?: number;
  maxDepth?: number;
  grammar?: string;
  start?: string;
};

declare module "zod" {
  interface GlobalMeta {
    weft?: WeftMeta;
  }
}
