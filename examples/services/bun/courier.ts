type Parcel = { order: string; customer: string; recipient: string; street: string };

const parcels: Parcel[] = [];

const get = async (base: string, path: string) => {
  const response = await fetch(`${Bun.env[base]}${path}`, { headers: { "x-service": "courier" } });
  const said = await response.json();
  if (!response.ok) throw new Error(`${path}: ${response.status} ${JSON.stringify(said)}`);
  return said;
};

export const state = () => ({ parcels });

export const ship = async ({ order }: { order: { id: string } }) => {
  const { customer, address_id } = await get("WEFT_ORDERS_URL", `/orders/${order.id}`);
  const { owner, street } = await get("WEFT_ADDRESSES_URL", `/addresses/${address_id}`);
  parcels.push({ order: order.id, customer, recipient: owner, street });
};

export const reset = () => {
  parcels.length = 0;
};
