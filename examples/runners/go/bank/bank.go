package bank

import "github.com/ahmedtwine/weft/sdk/go"

type Bank struct {
	Balance int `json:"balance"`
}

type Amount struct {
	Amount int `json:"amount" minimum:"1" maximum:"100"`
}

var bank Bank

func Reset() { bank = Bank{} }

func Deposit(given Amount) { bank.Balance += given.Amount }

func Withdraw(given Amount) {
	if given.Amount <= bank.Balance {
		bank.Balance -= given.Amount
	}
}

func Module() *weft.Module {
	weft.State("bank", func() Bank { return bank })
	return weft.NewModule().
		Function("reset", Reset).
		Function("deposit", Deposit).
		Function("withdraw", Withdraw)
}
