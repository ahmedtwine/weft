state = "closed"


def verb(name):
    return {"type": "object", "properties": {}, "additionalProperties": False, "weft": {"verb": name}}


doorSchema = {
    "type": "object",
    "properties": {"state": {"type": "string", "weft": {"read": "door.state"}}},
    "required": ["state"],
    "additionalProperties": False,
}
errorSchema = {"type": "string", "weft": {"read": "door.error"}}
openSchema = verb("open")
closeSchema = verb("close")
lockSchema = verb("lock")
unlockSchema = verb("unlock")


class Locked(Exception):
    pass


def door():
    return {"state": state}


def reset():
    global state
    state = "closed"


def open(_):
    global state
    if state == "locked":
        raise Locked("the door is locked")
    state = "open"


def close(_):
    global state
    state = "closed"


def lock(_):
    global state
    state = "locked"


def unlock(_):
    global state
    state = "closed"
