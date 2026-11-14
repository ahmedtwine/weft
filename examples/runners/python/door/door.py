from dataclasses import dataclass

import weft


@dataclass
class Door:
    state: str = "closed"
    error: str = ""


door = Door()
weft.state("door", lambda: door)


def verb(name):
    return {"type": "object", "properties": {}, "additionalProperties": False, "weft": {"verb": name}}


openSchema = verb("open")
closeSchema = verb("close")
lockSchema = verb("lock")
unlockSchema = verb("unlock")


def reset():
    global door
    door = Door()


def open(_):
    global door
    door = Door("locked", "the door is locked") if door.state == "locked" else Door("open")


def close(_):
    global door
    door = Door("closed")


def lock(_):
    global door
    door = Door("locked")


def unlock(_):
    global door
    door = Door("closed")
