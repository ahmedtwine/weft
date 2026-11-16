from dataclasses import dataclass


@dataclass
class Bank:
    balance: int = 0


@dataclass
class Amount:
    amount: int


bank = Bank()


def state():
    return bank


def reset():
    bank.balance = 0


def deposit(given: Amount):
    bank.balance += given.amount


def withdraw(given: Amount):
    if given.amount <= bank.balance:
        bank.balance -= given.amount
