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
import typing
from pathlib import Path

ADAPTER = "python"
START = "start"
PARENT = "--parent"


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
        for name in ("weft_diagram_join", "weft_diagram_turn"):
            getattr(self.lib, name).argtypes = [ctypes.c_void_p, _Text, ctypes.POINTER(_Text)]
        self.lib.weft_diagram_done.argtypes = [ctypes.c_void_p, _Text]
        self.ctx = self.lib.weft_context_new()

    def _checked(self, code):
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

@functools.cache
def _setup():
    return json.loads(os.environ["WEFT"])


@functools.cache
def _weft():
    return _Weft(_setup()["lib"])


def _native(value):
    if dataclasses.is_dataclass(value) and not isinstance(value, type):
        return dataclasses.asdict(value)
    if hasattr(value, "model_dump"):
        return value.model_dump(mode="json")
    if isinstance(value, (set, frozenset, tuple)):
        return list(value)
    raise TypeError(f"a function gives a {type(value).__name__}, and JSON has no such value")


def _json(value):
    return json.loads(json.dumps(value, default=_native))


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


def _given(kind, value):
    if dataclasses.is_dataclass(kind):
        return kind(**value)
    if hasattr(kind, "model_validate"):
        return kind.model_validate(value)
    return value


def _kind(function):
    names = list(inspect.signature(function).parameters)
    if len(names) > 1:
        raise RuntimeError(f"`{function.__name__}` takes at most one input")
    if not names:
        return None
    hints = typing.get_type_hints(function)
    if names[0] not in hints:
        raise RuntimeError(f"`{function.__name__}` names the type of its input")
    return hints[names[0]]


class _Runner:
    def __init__(self, code, adapter, lines):
        self.lines = lines
        self.calls = {}
        self.states = []
        self.serves = None
        for block in code:
            if block["adapter"] != adapter:
                continue
            for name, m in block["modules"].items():
                module = _imported(m["path"])
                lines.watch(module)

                def found(id, name=name, module=module):
                    function = getattr(module, id, None)
                    if not inspect.isfunction(function):
                        raise RuntimeError(f'`module "{name}"` maps `{id}`, and `{m["path"]}` has no function `{id}`')
                    return function

                for id in [m["reset"], *m["actions"].values()]:
                    if id is not None:
                        function = found(id)
                        self.calls[id] = (function, _kind(function))
                if m["state"] is not None:
                    self.states.append((name, found(m["state"])))
                if m["url"] is not None:
                    self.serves = str(found(m["url"])())
        self.start = {}
        self.covered = {}

    def _called(self, calls):
        result = None
        for call in calls:
            function, kind = self.calls[call["function"]]
            try:
                result = function() if kind is None else function(_given(kind, call["input"]))
            except Exception as error:
                return None, {"thrown": _fault(error)}
        return result, None

    def play(self, turn):
        step = turn.get("step")
        known = None
        if step is not None:
            known = self.start if step == START else self.covered.setdefault(step, {})
        self.lines.now = {} if known is not None else None
        try:
            result, thrown = self._called(turn["calls"])
        finally:
            for file, hit in (self.lines.now or {}).items():
                known.setdefault(file, set()).update(hit)
            self.lines.now = None
        if thrown is not None:
            return thrown
        try:
            states = {name: _json(read()) for name, read in self.states}
            return {"states": states, "result": _json(result)}
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
        report = {"adapter": _setup()["adapter"], "serves": runner.serves}
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
