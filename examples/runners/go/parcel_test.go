package main

import (
	"testing"

	"github.com/ahmedtwine/weft/examples/runners/go/bank"
	"github.com/ahmedtwine/weft/examples/runners/go/parcel"
	"github.com/ahmedtwine/weft/sdk/go"
)

func TestParcel(t *testing.T) {
	t.Cleanup(parcel.Stop)
	weft.Test(t, "tests/parcel.weft.kdl", bank.Module(), parcel.Module())
}
