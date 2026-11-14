package door

import "github.com/ahmedtwine/weft/sdk/go"

type Door struct {
	State string `json:"state"`
	Error string `json:"error"`
}

var door = Door{State: "closed"}

func Reset() { door = Door{State: "closed"} }

func Open() {
	if door.State == "locked" {
		door.Error = "the door is locked"
		return
	}
	door = Door{State: "open"}
}

func Close() { door = Door{State: "closed"} }

func Lock() { door = Door{State: "locked"} }

func Unlock() { door = Door{State: "closed"} }

func verb(name string) string {
	return `{"type":"object","properties":{},"additionalProperties":false,"weft":{"verb":"` + name + `"}}`
}

func Module() *weft.Module {
	weft.State("door", func() Door { return door })
	return weft.NewModule().
		Schema("openSchema", verb("open")).
		Schema("closeSchema", verb("close")).
		Schema("lockSchema", verb("lock")).
		Schema("unlockSchema", verb("unlock")).
		Function("reset", Reset).
		Function("open", Open).
		Function("close", Close).
		Function("lock", Lock).
		Function("unlock", Unlock)
}
