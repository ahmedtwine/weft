import { state } from "weft";
import { z } from "zod";

const bank = { balance: 0 };
state("bank", () => bank);

export const depositSchema = z.object({ amount: z.int().min(1).max(100) });
export const withdrawSchema = depositSchema;

export const reset = () => {
  bank.balance = 0;
};
export const deposit = ({ amount }: z.infer<typeof depositSchema>) => {
  bank.balance += amount;
};
export const withdraw = ({ amount }: z.infer<typeof withdrawSchema>) => {
  if (amount <= bank.balance) bank.balance -= amount;
};
