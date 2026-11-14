import { z } from "zod";

const TOOL = "service:tool";
const ANALYST = "user:acme-analyst";
const TENANT = "acme";

export const querySchema = z
  .strictObject({ store: z.literal("acme-store"), sql: z.literal("select * from orders") })
  .meta({ weft: { verb: "query" } });
export const stateSchema = z.object({
  tenant: z.string().meta({ weft: { read: "tool.tenant" } }),
});

const call = async (base: string, path: string, token: string, init: RequestInit = {}) => {
  const response = await fetch(`${Bun.env[base]}${path}`, {
    ...init,
    headers: { authorization: `Bearer ${token}`, "content-type": "application/json", ...init.headers },
  });
  const said = await response.json();
  if (!response.ok) throw new Error(`${path}: ${response.status} ${JSON.stringify(said)}`);
  return said;
};
const authz = (rpc: string, token: string, body: unknown) =>
  call("WEFT_AUTHZ_URL", `/${rpc}`, token, { method: "POST", body: JSON.stringify(body) });

export const query = async ({ store }: z.infer<typeof querySchema>) => {
  const object = { type: "store", id: store };
  const { allowed } = await authz("Check", ANALYST, { resource: object, permission: "query" });
  if (!allowed) throw new Error(`the store ${store} refuses the query`);
  const { values } = await authz("GetConfig", ANALYST, { object, keys: ["credential_ref"] });
  const ref: string = JSON.parse(values.credential_ref);
  const [, tenant, ...name] = ref.split("/");
  const { value } = await call("WEFT_VAULT_URL", `/v1/credentials/${name.join("/")}`, TOOL, {
    headers: { "x-tenant": tenant },
  });
  return { warehouse: value.host, rows: [{ tenant: value.tenant }] };
};

export const state = () => ({ tenant: TENANT });
