package door

import (
	"errors"

	"github.com/ahmedtwine/weft/sdk/go"
)

type Door struct {
	State string `json:"state"`
}

var state = "closed"

func Get() Door { return Door{State: state} }

func Reset() { state = "closed" }

func Open() error {
	if state == "locked" {
		return errors.New("the door is locked")
	}
	state = "open"
	return nil
}

func Close() { state = "closed" }

func Lock() { state = "locked" }

func Unlock() { state = "closed" }

func verb(name string) string {
	return `{"type":"object","properties":{},"additionalProperties":false,"weft":{"verb":"` + name + `"}}`
}

func Module() *weft.Module {
	return weft.NewModule().
		Schema("doorSchema", `{"type":"object","properties":{"state":{"type":"string","weft":{"read":"door.state"}}},"required":["state"],"additionalProperties":false}`).
		Schema("errorSchema", `{"type":"string","weft":{"read":"door.error"}}`).
		Schema("openSchema", verb("open")).
		Schema("closeSchema", verb("close")).
		Schema("lockSchema", verb("lock")).
		Schema("unlockSchema", verb("unlock")).
		Function("door", Get).
		Function("reset", Reset).
		Function("open", Open).
		Function("close", Close).
		Function("lock", Lock).
		Function("unlock", Unlock)
}
