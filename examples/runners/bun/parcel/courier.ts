import { state } from "weft";
import { z } from "zod";

const Parcel = z
  .object({ order: z.string(), customer: z.string(), recipient: z.string(), street: z.string() })
  .meta({ title: "Parcel" });

const parcels: z.infer<typeof Parcel>[] = [];
state("courier.parcels", () => parcels, z.array(Parcel));

export const shipSchema = z.strictObject({ order: z.literal("o-ana") });

const get = async (base: string, path: string) => {
  const response = await fetch(`${Bun.env[base]}${path}`, { headers: { "x-service": "courier" } });
  const said = await response.json();
  if (!response.ok) throw new Error(`${path}: ${response.status} ${JSON.stringify(said)}`);
  return said;
};

export const ship = async ({ order }: z.infer<typeof shipSchema>) => {
  const { customer, address_id } = await get("WEFT_ORDERS_URL", `/orders/${order}`);
  const { owner, street } = await get("WEFT_ADDRESSES_URL", `/addresses/${address_id}`);
  parcels.push({ order, customer, recipient: owner, street });
};

export const reset = () => {
  parcels.length = 0;
};
