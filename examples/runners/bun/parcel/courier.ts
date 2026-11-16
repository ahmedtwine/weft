import { Pool, state, type TestCase } from "weft";
import { z } from "zod";

const Parcel = z
  .object({ order: z.string(), customer: z.string(), recipient: z.string(), street: z.string() })
  .meta({ title: "Parcel" });

const parcels: z.infer<typeof Parcel>[] = [];
state("courier.parcels", () => parcels, z.array(Parcel));

const get = async (base: string, path: string) => {
  const response = await fetch(`${Bun.env[base]}${path}`, { headers: { "x-service": "courier" } });
  const said = await response.json();
  if (!response.ok) throw new Error(`${path}: ${response.status} ${JSON.stringify(said)}`);
  return said;
};

const Order = z.object({ id: z.string(), customer: z.string() });

export const ship = async (_: undefined, tc: TestCase) => {
  const { id } = tc.draw(new Pool<z.infer<typeof Order>>(tc, "orders").valuesConsumed());
  const { customer, address_id } = await get("WEFT_ORDERS_URL", `/orders/${id}`);
  const { owner, street } = await get("WEFT_ADDRESSES_URL", `/addresses/${address_id}`);
  parcels.push({ order: id, customer, recipient: owner, street });
  tc.target(parcels.length, "parcels");
};

export const reset = () => {
  parcels.length = 0;
};
