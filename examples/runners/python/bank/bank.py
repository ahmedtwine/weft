from dataclasses import dataclass
from typing import Annotated

import weft


@dataclass
class Bank:
    balance: int = 0


@dataclass
class Amount:
    amount: Annotated[int, range(1, 101)]


bank = Bank()
weft.state("bank", lambda: bank)


def reset():
    bank.balance = 0


def deposit(given: Amount):
    bank.balance += given.amount


def withdraw(given: Amount):
    if given.amount <= bank.balance:
        bank.balance -= given.amount
