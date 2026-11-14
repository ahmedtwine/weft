package main

import (
	"os"

	"github.com/ahmedtwine/weft/examples/runners/go/door"
	"github.com/ahmedtwine/weft/sdk/go"
)

func main() {
	os.Exit(weft.Run(door.Module()))
}
