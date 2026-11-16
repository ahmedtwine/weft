package main

import (
	"testing"

	"github.com/ahmedtwine/weft/examples/services/go/orders"
	"github.com/ahmedtwine/weft/sdk/go"
)

func TestParcel(t *testing.T) {
	t.Cleanup(orders.Stop)
	weft.Test(t, "tests/parcel.weft.kdl", functions...)
}
