package main

import (
	"os"

	"github.com/ahmedtwine/weft/examples/apps/orders/orders"
	"github.com/ahmedtwine/weft/sdk/go"
)

func main() {
	os.Exit(weft.Run(orders.Module()))
}
