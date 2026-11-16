const bank = { balance: 0 };

export const state = () => bank;
export const reset = () => {
  bank.balance = 0;
};
export const deposit = ({ amount }: { amount: number }) => {
  bank.balance += amount;
};
export const withdraw = ({ amount }: { amount: number }) => {
  if (amount <= bank.balance) bank.balance -= amount;
};
