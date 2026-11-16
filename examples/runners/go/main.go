package main

import (
	"os"

	"github.com/ahmedtwine/weft/examples/services/go/bank"
	"github.com/ahmedtwine/weft/examples/services/go/orders"
	"github.com/ahmedtwine/weft/sdk/go"
)

var functions = []any{
	bank.Reset, bank.State, bank.Deposit, bank.Withdraw,
	orders.URL, orders.Reset, orders.State, orders.Cancel,
}

func main() {
	code := weft.Run(functions...)
	orders.Stop()
	os.Exit(code)
}
