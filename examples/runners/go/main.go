package main

import (
	"os"

	"github.com/ahmedtwine/weft/examples/runners/go/door"
	"github.com/ahmedtwine/weft/examples/runners/go/parcel"
	"github.com/ahmedtwine/weft/sdk/go"
)

func main() {
	code := weft.Run(door.Module(), parcel.Module())
	parcel.Stop()
	os.Exit(code)
}
