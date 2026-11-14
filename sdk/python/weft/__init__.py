import ctypes
import dataclasses
import importlib.util
import inspect
import json
import os
import sys
import traceback
import types
import typing
from pathlib import Path

ADAPTER = "python"
START = "start"
RESET = "reset"
URL = "url"


class _Text(ctypes.Structure):
    _fields_ = [("data", ctypes.c_void_p), ("len", ctypes.c_size_t)]


class _Weft:
    def __init__(self, library):
        self.lib = ctypes.CDLL(library)
        self.lib.weft_context_new.restype = ctypes.c_void_p
        self.lib.weft_context_last_error.restype = _Text
        self.lib.weft_context_last_error.argtypes = [ctypes.c_void_p]
        self.lib.weft_owned_string_free.argtypes = [ctypes.c_void_p, ctypes.POINTER(_Text)]
        for name in ("weft_diagram_join", "weft_diagram_turn"):
            getattr(self.lib, name).argtypes = [ctypes.c_void_p, _Text, ctypes.POINTER(_Text)]
        self.lib.weft_diagram_done.argtypes = [ctypes.c_void_p, _Text]
        self.ctx = self.lib.weft_context_new()

    def _checked(self, code):
        if code != 0:
            said = self.lib.weft_context_last_error(self.ctx)
            raise RuntimeError(ctypes.string_at(said.data, said.len).decode())

    def _asked(self, call, text):
        data = text.encode()
        out = _Text()
        self._checked(call(self.ctx, _Text(ctypes.cast(data, ctypes.c_void_p), len(data)), ctypes.byref(out)))
        said = ctypes.string_at(out.data, out.len).decode()
        self.lib.weft_owned_string_free(self.ctx, ctypes.byref(out))
        return json.loads(said)

    def join(self, name):
        return self._asked(self.lib.weft_diagram_join, name)

    def turn(self, report):
        return self._asked(self.lib.weft_diagram_turn, json.dumps(report))

    def done(self, said):
        data = json.dumps(said).encode()
        self._checked(self.lib.weft_diagram_done(self.ctx, _Text(ctypes.cast(data, ctypes.c_void_p), len(data))))


_STATES = {}


def state(key, read):
    try:
        hint = typing.get_type_hints(read).get("return") if callable(read) else None
    except Exception:
        hint = None
    _STATES[key] = (read if callable(read) else (lambda: read), hint)


def _shape(kind):
    origin, args = typing.get_origin(kind), typing.get_args(kind)
    if origin is typing.Literal:
        return {"enum": list(args)}
    if origin in (typing.Union, types.UnionType):
        return next((_shape(a) for a in args if a is not type(None)), {})
    if kind is bool:
        return {"type": "boolean"}
    if kind is int:
        return {"type": "integer"}
    if kind is float:
        return {"type": "number"}
    if kind is str:
        return {"type": "string"}
    if hasattr(kind, "model_json_schema"):
        return kind.model_json_schema()
    if dataclasses.is_dataclass(kind) or typing.is_typeddict(kind):
        hints = typing.get_type_hints(kind)
        needed = (
            [f.name for f in dataclasses.fields(kind) if f.default is dataclasses.MISSING and f.default_factory is dataclasses.MISSING]
            if dataclasses.is_dataclass(kind)
            else sorted(kind.__required_keys__)
        )
        return {
            "type": "object",
            "title": kind.__name__,
            "properties": {k: _shape(v) for k, v in hints.items()},
            "required": needed,
            "additionalProperties": False,
        }
    if kind in (list, set, frozenset, tuple) or origin in (list, set, frozenset, tuple):
        return {"type": "array", "items": _shape(args[0]) if args else {}}
    if kind is dict or origin is dict:
        return {"type": "object", "additionalProperties": _shape(args[1]) if len(args) > 1 else {}}
    return {}


def _guessed(value):
    if dataclasses.is_dataclass(value) or hasattr(value, "model_json_schema"):
        return _shape(type(value))
    if isinstance(value, dict):
        return {"type": "object", "properties": {k: _guessed(v) for k, v in value.items()}}
    if isinstance(value, (list, set, frozenset, tuple)):
        return {"type": "array", "items": _guessed(next(iter(value))) if value else {}}
    return _shape(type(value))


def _schemas():
    out = {}
    for key, (read, hint) in _STATES.items():
        try:
            out[key] = _shape(hint) if hint is not None else _guessed(read())
        except Exception:
            out[key] = {}
    return out


def _native(value):
    if dataclasses.is_dataclass(value) and not isinstance(value, type):
        return dataclasses.asdict(value)
    if hasattr(value, "model_dump"):
        return value.model_dump(mode="json")
    if isinstance(value, (set, frozenset, tuple)):
        return list(value)
    raise TypeError(f"weft.state reads a {type(value).__name__}, and JSON has no such value")


def _states():
    return {key: json.loads(json.dumps(read(), default=_native)) for key, (read, _) in _STATES.items()}


class _Lines:
    def __init__(self):
        self.files = {}
        self.now = None
        monitoring = sys.monitoring
        self.tool = next(i for i in range(6) if monitoring.get_tool(i) is None)
        monitoring.use_tool_id(self.tool, "weft")
        monitoring.register_callback(self.tool, monitoring.events.LINE, self._line)

    def _line(self, code, line):
        if self.now is not None and code.co_filename in self.files:
            self.now.setdefault(code.co_filename, set()).add(line)

    def watch(self, module):
        file = module.__file__
        if file in self.files:
            return
        lines = set()
        source = compile(Path(file).read_text(), file, "exec")
        stack = [source]
        while stack:
            code = stack.pop()
            lines.update(line for _, _, line in code.co_lines() if line is not None)
            stack.extend(c for c in code.co_consts if hasattr(c, "co_lines"))
        self.files[file] = sorted(lines)
        for value in vars(module).values():
            for code in _codes(value):
                sys.monitoring.set_local_events(self.tool, code, sys.monitoring.events.LINE)

    def spans(self, ran):
        cwd = Path.cwd()
        out = []
        for file, hit in ran.items():
            shown = os.path.relpath(file, cwd)
            open_span = None
            for line in self.files[file]:
                if line not in hit:
                    open_span = None
                elif open_span:
                    open_span["to"] = line
                else:
                    open_span = {"file": shown, "from": line, "to": line}
                    out.append(open_span)
        return out


def _codes(value):
    seen = []
    stack = [getattr(value, "__code__", None)]
    if isinstance(value, type):
        stack.extend(getattr(v, "__code__", None) for v in vars(value).values())
    while stack:
        code = stack.pop()
        if code is None or code in seen:
            continue
        seen.append(code)
        stack.extend(c for c in code.co_consts if hasattr(c, "co_lines"))
    return seen


def _imported(place):
    path = Path(place).resolve()
    spec = importlib.util.spec_from_file_location(f"weft_{path.stem}_{abs(hash(path))}", path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def _fault(error):
    frames = traceback.extract_tb(error.__traceback__)
    at = f" at {os.path.relpath(frames[-1].filename)}:{frames[-1].lineno}" if frames else ""
    return f"{type(error).__name__}: {error}{at}"


def _input(function):
    parameters = list(inspect.signature(function).parameters.values())
    if not parameters:
        return None
    hints = typing.get_type_hints(function)
    if parameters[0].name not in hints:
        raise RuntimeError(f"`{function.__name__}` names the type of its input")
    return hints[parameters[0].name]


def _given(kind, value):
    if dataclasses.is_dataclass(kind):
        return kind(**value)
    if hasattr(kind, "model_validate"):
        return kind.model_validate(value)
    return value


def _functions(module):
    found = [
        value
        for name, value in vars(module).items()
        if inspect.isfunction(value) and value.__module__ == module.__name__ and not name.startswith("_")
    ]
    return sorted(found, key=lambda f: f.__code__.co_firstlineno)


class _Runner:
    def __init__(self, code, adapter, lines):
        self.lines = lines
        self.reset = []
        self.actions = []
        self.said_reset = []
        self.said_actions = []
        self.serves = None
        for block in code:
            if block["adapter"] != adapter:
                continue
            for name, place in block["modules"].items():
                module = _imported(place)
                lines.watch(module)
                for function in _functions(module):
                    kind = _input(function)
                    said = {"function": [name, function.__name__], "input": None if kind is None else _shape(kind)}
                    if function.__name__ == URL:
                        self.serves = str(function())
                    elif function.__name__ == RESET:
                        self.reset.append((function, kind))
                        self.said_reset.append(said)
                    else:
                        self.actions.append((function, kind))
                        self.said_actions.append(said)
        self.start = {}
        self.covered = {}

    def _reported(self, calls, step):
        for call in calls:
            function, kind = (self.reset if step == START else self.actions)[call["use"]]
            try:
                function() if kind is None else function(_given(kind, call["input"]))
            except Exception as error:
                return {"thrown": _fault(error)}
        return None

    def play(self, turn):
        step = turn.get("step")
        known = None
        if step is not None:
            known = self.start if step == START else self.covered.setdefault(step, {})
        self.lines.now = {} if known is not None else None
        try:
            thrown = self._reported(turn["calls"], step)
        finally:
            for file, hit in (self.lines.now or {}).items():
                known.setdefault(file, set()).update(hit)
            self.lines.now = None
        if thrown is not None:
            return thrown
        try:
            return {"states": _states()}
        except Exception as error:
            return {"thrown": _fault(error)}

    def ran(self):
        return {
            "start": self.lines.spans(self.start),
            "actions": {verb: self.lines.spans(ran) for verb, ran in self.covered.items()},
        }


def run():
    weft = _Weft(os.environ["WEFT_LIB"])
    try:
        runner = _Runner(
            weft.join(os.environ["WEFT_DIAGRAM"]),
            os.environ.get("WEFT_ADAPTER", ADAPTER),
            _Lines(),
        )
        report = {
            "adapter": os.environ.get("WEFT_ADAPTER", ADAPTER),
            "start": runner.said_reset,
            "actions": runner.said_actions,
            "serves": runner.serves,
            "states": _schemas(),
        }
        while not (turn := weft.turn(report)).get("done"):
            report = runner.play(turn)
        said = {"ran": runner.ran()}
    except Exception:
        said = {"error": traceback.format_exc()}
    weft.done(said)
    return 0 if "ran" in said else 1
