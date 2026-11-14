package main

import (
	"os"

	"github.com/ahmedtwine/weft/examples/runners/go/bank"
	"github.com/ahmedtwine/weft/examples/runners/go/parcel"
	"github.com/ahmedtwine/weft/sdk/go"
)

func main() {
	code := weft.Run(bank.Module(), parcel.Module())
	parcel.Stop()
	os.Exit(code)
}
