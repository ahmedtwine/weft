import ctypes
import dataclasses
import functools
import importlib.util
import inspect
import json
import os
import subprocess
import sys
import traceback
import types
import typing
from pathlib import Path

ADAPTER = "python"
START = "start"
RESET = "reset"
URL = "url"
PARENT = "--parent"
STOP_TEST = -5
ASSUME = -6


class StopTest(Exception):
    cause = "stop_test"


class AssumeFailed(Exception):
    cause = "assume"


class _Text(ctypes.Structure):
    _fields_ = [("data", ctypes.c_void_p), ("len", ctypes.c_size_t)]


def _text(value):
    data = value.encode()
    text = _Text(ctypes.cast(data, ctypes.c_void_p), len(data))
    text.kept = data
    return text


class _Weft:
    def __init__(self, library):
        self.lib = ctypes.CDLL(library)
        self.lib.weft_context_new.restype = ctypes.c_void_p
        self.lib.weft_context_last_error.restype = _Text
        self.lib.weft_context_last_error.argtypes = [ctypes.c_void_p]
        self.lib.weft_owned_string_free.argtypes = [ctypes.c_void_p, ctypes.POINTER(_Text)]
        for name in ("weft_diagram_join", "weft_diagram_turn", "weft_generate"):
            getattr(self.lib, name).argtypes = [ctypes.c_void_p, _Text, ctypes.POINTER(_Text)]
        self.lib.weft_diagram_done.argtypes = [ctypes.c_void_p, _Text]
        self.lib.weft_pool_add.argtypes = [ctypes.c_void_p, _Text, _Text]
        self.lib.weft_pool_generate.argtypes = [ctypes.c_void_p, _Text, ctypes.c_bool, ctypes.POINTER(_Text)]
        self.lib.weft_target.argtypes = [ctypes.c_void_p, ctypes.c_double, _Text]
        self.ctx = self.lib.weft_context_new()

    def _checked(self, code):
        if code == STOP_TEST:
            raise StopTest()
        if code == ASSUME:
            raise AssumeFailed()
        if code != 0:
            said = self.lib.weft_context_last_error(self.ctx)
            raise RuntimeError(ctypes.string_at(said.data, said.len).decode())

    def _asked(self, call, *given):
        out = _Text()
        self._checked(call(self.ctx, *given, ctypes.byref(out)))
        said = ctypes.string_at(out.data, out.len).decode()
        self.lib.weft_owned_string_free(self.ctx, ctypes.byref(out))
        return json.loads(said)

    def join(self, name):
        return self._asked(self.lib.weft_diagram_join, _text(name))

    def turn(self, report):
        return self._asked(self.lib.weft_diagram_turn, _text(json.dumps(report)))

    def done(self, said):
        self._checked(self.lib.weft_diagram_done(self.ctx, _text(json.dumps(said))))

    def generate(self, schema):
        return self._asked(self.lib.weft_generate, _text(json.dumps(schema)))

    def pool_add(self, pool, value):
        self._checked(self.lib.weft_pool_add(self.ctx, _text(pool), _text(json.dumps(value, default=_native))))

    def pool_generate(self, pool, consume):
        return self._asked(self.lib.weft_pool_generate, _text(pool), consume)

    def target(self, value, label):
        self._checked(self.lib.weft_target(self.ctx, value, _text(label)))


@functools.cache
def _setup():
    return json.loads(os.environ["WEFT"])


@functools.cache
def _weft():
    return _Weft(_setup()["lib"])


class Generator:
    def __init__(self, do_draw):
        self.do_draw = do_draw


class TestCase:
    def draw(self, generator):
        if isinstance(generator, Generator):
            return generator.do_draw(self)
        return _given(generator, _weft().generate(_shape(generator)))

    def assume(self, condition):
        if not condition:
            raise AssumeFailed()

    def target(self, score, label):
        _weft().target(score, label)


class Pool:
    def __init__(self, tc, name):
        self.name = name

    def add(self, value):
        _weft().pool_add(self.name, value)

    def values_reusable(self):
        return Generator(lambda tc: _weft().pool_generate(self.name, False))

    def values_consumed(self):
        return Generator(lambda tc: _weft().pool_generate(self.name, True))


_STATES = {}


def state(key, read):
    try:
        hint = typing.get_type_hints(read).get("return") if callable(read) else None
    except Exception:
        hint = None
    _STATES[key] = (read if callable(read) else (lambda: read), hint)


def _shape(kind):
    origin, args = typing.get_origin(kind), typing.get_args(kind)
    if origin is typing.Annotated:
        shape = _shape(args[0])
        for bound in args[1:]:
            if isinstance(bound, range):
                shape |= {"minimum": bound.start, "maximum": bound.stop - 1}
        return shape
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
        hints = typing.get_type_hints(kind, include_extras=True)
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


def _kinds(function):
    hints = typing.get_type_hints(function)
    names = list(inspect.signature(function).parameters)
    if any(name not in hints for name in names):
        raise RuntimeError(f"`{function.__name__}` names the type of its input")
    return [hints[name] for name in names]


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
        self.tc = TestCase()
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
                    kinds = _kinds(function)
                    kind = next((kind for kind in kinds if kind is not TestCase), None)
                    said = {"function": [name, function.__name__], "input": None if kind is None else _shape(kind)}
                    if function.__name__ == URL:
                        self.serves = str(function())
                    elif function.__name__ == RESET:
                        self.reset.append((function, kinds))
                        self.said_reset.append(said)
                    else:
                        self.actions.append((function, kinds))
                        self.said_actions.append(said)
        self.start = {}
        self.covered = {}

    def _reported(self, calls, step):
        for call in calls:
            function, kinds = (self.reset if step == START else self.actions)[call["use"]]
            try:
                function(*(self.tc if kind is TestCase else _given(kind, call["input"]) for kind in kinds))
            except (StopTest, AssumeFailed) as stop:
                return {"refused": stop.cause}
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
    weft = _weft()
    try:
        runner = _Runner(
            weft.join(_setup()["diagram"]),
            _setup()["adapter"],
            _Lines(),
        )
        report = {
            "adapter": _setup()["adapter"],
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


def test(file):
    weft = subprocess.Popen(
        [os.environ.get("WEFT_BIN", "weft"), PARENT, ADAPTER, file],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        text=True,
    )
    first = weft.stdout.readline()
    joins = first.startswith("{")
    if joins:
        given = json.loads(first)
        os.chdir(given["cwd"])
        os.environ.update(given["env"])
        run()
    report = ("" if joins else first) + weft.stdout.read()
    code = weft.wait()
    if code != 0:
        raise AssertionError(report or f"weft ends with {code}")
    print(report)
