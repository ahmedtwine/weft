package main

import (
	"os"

	"github.com/ahmedtwine/weft/examples/apps/authz/authz"
	"github.com/ahmedtwine/weft/sdk/go"
)

func main() {
	os.Exit(weft.Run(authz.Module()))
}
