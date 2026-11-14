import ctypes
import dataclasses
import importlib.util
import json
import os
import sys
import traceback
from pathlib import Path

ADAPTER = "python"
START = "start"


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
    _STATES[key] = read if callable(read) else (lambda: read)


def _native(value):
    if dataclasses.is_dataclass(value) and not isinstance(value, type):
        return dataclasses.asdict(value)
    if hasattr(value, "model_dump"):
        return value.model_dump(mode="json")
    if isinstance(value, (set, frozenset, tuple)):
        return list(value)
    raise TypeError(f"weft.state reads a {type(value).__name__}, and JSON has no such value")


def _states():
    return {key: json.loads(json.dumps(read(), default=_native)) for key, read in _STATES.items()}


def _schema(value):
    if value is None:
        return None
    if hasattr(value, "model_json_schema"):
        return value.model_json_schema()
    return value


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


class _Runner:
    def __init__(self, code, adapter, lines):
        self.lines = lines
        self.uses = []
        self.hello = []
        self.serves = None
        for block in code:
            if block["adapter"] != adapter:
                self.uses.extend(None for _ in block["uses"])
                self.hello.extend(None for _ in block["uses"])
                continue
            modules = {name: _imported(place) for name, place in block["modules"].items()}
            for module in modules.values():
                lines.watch(module)

            def exported(name):
                module, export = name
                if not hasattr(modules[module], export):
                    raise RuntimeError(f"`{module}` exports no `{export}`")
                return getattr(modules[module], export)

            def schema(name):
                return (".".join(name), _schema(exported(name))) if name else (None, None)

            if block.get("serve"):
                given = exported(block["serve"])
                self.serves = str(given() if callable(given) else given)
            for line in block["uses"]:
                run = None if line["from"] else exported(line["function"])
                if run is not None and not callable(run):
                    raise RuntimeError(f"`{'.'.join(line['function'])}` is no function")
                inputs = schema(line["input"])
                self.hello.append({"input": inputs[1]})
                self.uses.append({"run": run, "method": line["function"][1], "input": inputs[0]})
        self.pool = {}
        self.start = {}
        self.actions = {}

    def _target(self, use, source):
        if use["run"] is not None:
            return use["run"]
        value = self.pool[source]
        return getattr(value, use["method"]) if use["method"] else value

    def _called(self, use, call):
        target = self._target(use, call["from"])
        try:
            if use["input"] is None:
                return {"result": target()}
            return {"result": target(call["input"])}
        except Exception as error:
            return {"thrown": _fault(error)}

    def _reported(self, calls):
        added = []
        for call in calls:
            use = self.uses[call["use"]]
            if use is None:
                raise RuntimeError(f"the diagram has no line {call['use']} for this runner")
            ended = self._called(use, call)
            if "thrown" in ended:
                return {"thrown": ended["thrown"]}
            if call["add"] is not None:
                self.pool[call["add"]] = ended["result"]
                added.append(call["add"])
        return {"added": added}

    def play(self, turn):
        step = turn.get("step")
        if step == START:
            self.pool.clear()
        known = None
        if step is not None:
            known = self.start if step == START else self.actions.setdefault(step, {})
        self.lines.now = {} if known is not None else None
        try:
            report = self._reported(turn["calls"])
        finally:
            for file, hit in (self.lines.now or {}).items():
                known.setdefault(file, set()).update(hit)
            self.lines.now = None
        if "added" not in report:
            return report
        try:
            return {"states": _states(), **report}
        except Exception as error:
            return {"thrown": _fault(error)}

    def ran(self):
        return {
            "start": self.lines.spans(self.start),
            "actions": {verb: self.lines.spans(ran) for verb, ran in self.actions.items()},
        }


def run():
    weft = _Weft(os.environ["WEFT_LIB"])
    try:
        runner = _Runner(
            weft.join(os.environ["WEFT_DIAGRAM"]),
            os.environ.get("WEFT_ADAPTER", ADAPTER),
            _Lines(),
        )
        report = {"uses": runner.hello, "serves": runner.serves, "states": list(_STATES)}
        while not (turn := weft.turn(report)).get("done"):
            report = runner.play(turn)
        said = {"ran": runner.ran()}
    except Exception:
        said = {"error": traceback.format_exc()}
    weft.done(said)
    return 0 if "ran" in said else 1
