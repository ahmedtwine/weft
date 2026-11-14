export type WeftMeta = {
  verb?: string;
  pool?: string;
  from?: string;
  consume?: boolean;
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
