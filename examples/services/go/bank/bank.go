package bank

type Bank struct {
	Balance int `json:"balance"`
}

type Amount struct {
	Amount int `json:"amount"`
}

var bank Bank

func Reset() { bank = Bank{} }

func State() Bank { return bank }

func Deposit(given Amount) { bank.Balance += given.Amount }

func Withdraw(given Amount) {
	if given.Amount <= bank.Balance {
		bank.Balance -= given.Amount
	}
}
