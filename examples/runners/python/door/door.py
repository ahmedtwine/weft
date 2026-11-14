from dataclasses import dataclass

import weft


@dataclass
class Door:
    state: str = "closed"
    error: str = ""


door = Door()
weft.state("door", lambda: door)


def reset():
    global door
    door = Door()


def open():
    global door
    door = Door("locked", "the door is locked") if door.state == "locked" else Door("open")


def close():
    global door
    door = Door("closed")


def lock():
    global door
    door = Door("locked")


def unlock():
    global door
    door = Door("closed")
