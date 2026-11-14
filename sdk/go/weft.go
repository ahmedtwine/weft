package weft

import (
	"encoding/json"
	"errors"
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"slices"
	"strings"
)

const (
	adapter = "go"
	start   = "start"
)

type export struct {
	function reflect.Value
	schema   json.RawMessage
}

type Module struct {
	file    string
	exports map[string]export
}

func NewModule() *Module {
	_, file, _, _ := runtime.Caller(1)
	if found, err := filepath.EvalSymlinks(file); err == nil {
		file = found
	}
	return &Module{file: file, exports: map[string]export{}}
}

func (m *Module) Schema(name, schema string) *Module {
	m.exports[name] = export{schema: json.RawMessage(schema)}
	return m
}

func (m *Module) Function(name string, function any) *Module {
	value := reflect.ValueOf(function)
	if value.Kind() != reflect.Func || value.Type().NumIn() > 1 || value.Type().NumOut() > 2 {
		panic(fmt.Sprintf("weft: `%s` is a func with at most one input and two results", name))
	}
	m.exports[name] = export{function: value}
	return m
}

type used struct {
	function reflect.Value
	module   int
	input    bool
}

type read struct {
	key  string
	read func() any
}

var states []read

func State[T any](key string, get func() T) {
	for i, known := range states {
		if known.key == key {
			states = append(states[:i], states[i+1:]...)
			break
		}
	}
	states = append(states, read{key: key, read: func() any { return get() }})
}

func observed() (result map[string]any, fault string) {
	defer func() {
		if recover() != nil {
			fault = panicked()
		}
	}()
	result = map[string]any{}
	for _, s := range states {
		result[s.key] = s.read()
	}
	return result, ""
}

type runner struct {
	serves  string
	modules []*Module
	uses    []*used
	hello   []any
	start   map[int]map[string]bool
	actions map[string]map[int]map[string]bool
}

type line struct {
	Role     string     `json:"role"`
	Function [2]string  `json:"function"`
	Input    *[2]string `json:"input"`
	From     bool       `json:"from"`
}

type block struct {
	Adapter string            `json:"adapter"`
	Modules map[string]string `json:"modules"`
	Uses    []line            `json:"uses"`
	Serve   *[2]string        `json:"serve"`
}

type failure struct {
	refused bool
	fault   bool
	message string
}

var errType = reflect.TypeFor[error]()

func place(at string) string {
	path, _ := filepath.Abs(at)
	if found, err := filepath.EvalSymlinks(path); err == nil {
		return found
	}
	return path
}

func newRunner(code []block, name string, modules []*Module) (*runner, error) {
	r := &runner{modules: modules, start: map[int]map[string]bool{}, actions: map[string]map[int]map[string]bool{}}
	for _, b := range code {
		if b.Adapter != name {
			for range b.Uses {
				r.uses = append(r.uses, nil)
				r.hello = append(r.hello, nil)
			}
			continue
		}
		found := map[string]int{}
		for module, at := range b.Modules {
			index := slices.IndexFunc(modules, func(m *Module) bool { return m.file == place(at) })
			if index < 0 {
				return nil, fmt.Errorf("the runner has no module `%s`", at)
			}
			found[module] = index
		}
		exported := func(n [2]string) (int, export, error) {
			index, ok := found[n[0]]
			if !ok {
				return 0, export{}, fmt.Errorf("`impl %q` has no `module %q`", name, n[0])
			}
			e, ok := modules[index].exports[n[1]]
			if !ok {
				return 0, export{}, fmt.Errorf("`%s` exports no `%s`", n[0], n[1])
			}
			return index, e, nil
		}
		schema := func(n *[2]string) (string, json.RawMessage, error) {
			if n == nil {
				return "", nil, nil
			}
			_, e, err := exported(*n)
			if err == nil && e.schema == nil {
				err = fmt.Errorf("`%s.%s` is no schema", n[0], n[1])
			}
			return n[0] + "." + n[1], e.schema, err
		}
		if b.Serve != nil {
			_, e, err := exported(*b.Serve)
			if err != nil {
				return nil, err
			}
			result, fail := called(e.function, nil)
			if fail != nil {
				return nil, errors.New(fail.message)
			}
			r.serves = fmt.Sprint(result)
		}
		for _, l := range b.Uses {
			if l.From {
				return nil, errors.New("the go runner calls named functions, and a `from` or `consume` line needs a runner with objects")
			}
			index, e, err := exported(l.Function)
			if err != nil {
				return nil, err
			}
			if !e.function.IsValid() {
				return nil, fmt.Errorf("`%s.%s` is no function", l.Function[0], l.Function[1])
			}
			_, input, err := schema(l.Input)
			if err != nil {
				return nil, err
			}
			r.hello = append(r.hello, map[string]json.RawMessage{"input": input})
			r.uses = append(r.uses, &used{function: e.function, module: index, input: input != nil})
		}
	}
	return r, nil
}

func called(function reflect.Value, input json.RawMessage) (result any, fail *failure) {
	defer func() {
		if p := recover(); p != nil {
			fail = &failure{fault: true, message: fmt.Sprintf("panic: %v%s", p, panicked())}
		}
	}()
	kind := function.Type()
	var args []reflect.Value
	if kind.NumIn() == 1 {
		value := reflect.New(kind.In(0))
		if len(input) > 0 && string(input) != "null" {
			if err := json.Unmarshal(input, value.Interface()); err != nil {
				return nil, &failure{refused: true}
			}
		}
		args = append(args, value.Elem())
	}
	outs := function.Call(args)
	if n := len(outs); n > 0 && kind.Out(n-1) == errType {
		if err, _ := outs[n-1].Interface().(error); err != nil {
			return nil, &failure{message: err.Error()}
		}
		outs = outs[:n-1]
	}
	if len(outs) == 1 {
		return outs[0].Interface(), nil
	}
	return nil, nil
}

func panicked() string {
	pcs := make([]uintptr, 32)
	frames := runtime.CallersFrames(pcs[:runtime.Callers(4, pcs)])
	for {
		frame, more := frames.Next()
		if !strings.HasPrefix(frame.Function, "runtime.") && !strings.Contains(frame.Function, "reflect.") {
			cwd, _ := os.Getwd()
			file, err := filepath.Rel(cwd, frame.File)
			if err != nil {
				file = frame.File
			}
			return fmt.Sprintf(" at %s:%d", file, frame.Line)
		}
		if !more {
			return ""
		}
	}
}

type call struct {
	Use   int             `json:"use"`
	Input json.RawMessage `json:"input"`
	Add   *uint64         `json:"add"`
}

type turn struct {
	Step  *string `json:"step"`
	Calls []call  `json:"calls"`
	Done  bool    `json:"done"`
}

func (r *runner) note(ran map[int]map[string]bool, u *used) {
	if ran[u.module] == nil {
		ran[u.module] = map[string]bool{}
	}
	name := runtime.FuncForPC(u.function.Pointer()).Name()
	ran[u.module][name[strings.LastIndex(name, ".")+1:]] = true
}

func (r *runner) play(t turn) (any, error) {
	ran := map[int]map[string]bool{}
	switch {
	case t.Step == nil:
	case *t.Step == start:
		ran = r.start
	default:
		if r.actions[*t.Step] == nil {
			r.actions[*t.Step] = map[int]map[string]bool{}
		}
		ran = r.actions[*t.Step]
	}
	added := []uint64{}
	for _, c := range t.Calls {
		if c.Use >= len(r.uses) || r.uses[c.Use] == nil {
			return nil, fmt.Errorf("the diagram has no line %d for this runner", c.Use)
		}
		u := r.uses[c.Use]
		r.note(ran, u)
		input := c.Input
		if !u.input {
			input = nil
		}
		_, fail := called(u.function, input)
		switch {
		case fail == nil:
			if c.Add != nil {
				added = append(added, *c.Add)
			}
		case fail.refused:
			return map[string]bool{"refused": true}, nil
		default:
			return map[string]string{"thrown": fail.message}, nil
		}
	}
	found, fault := observed()
	if fault != "" {
		return map[string]string{"thrown": fault}, nil
	}
	return map[string]any{"states": found, "added": added}, nil
}


type span struct {
	File string `json:"file"`
	From int    `json:"from"`
	To   int    `json:"to"`
}

func (r *runner) spans(ran map[int]map[string]bool) []span {
	cwd, _ := os.Getwd()
	cwd = place(cwd)
	out := []span{}
	for index, names := range ran {
		file := r.modules[index].file
		set := token.NewFileSet()
		parsed, err := parser.ParseFile(set, file, nil, parser.SkipObjectResolution)
		if err != nil {
			continue
		}
		shown, err := filepath.Rel(cwd, file)
		if err != nil {
			shown = file
		}
		for _, decl := range parsed.Decls {
			if f, ok := decl.(*ast.FuncDecl); ok && f.Recv == nil && names[f.Name.Name] {
				out = append(out, span{File: shown, From: set.Position(f.Pos()).Line, To: set.Position(f.End()).Line})
			}
		}
	}
	slices.SortFunc(out, func(a, b span) int { return strings.Compare(fmt.Sprint(a), fmt.Sprint(b)) })
	return out
}

func (r *runner) ran() map[string]any {
	actions := map[string][]span{}
	for verb, ran := range r.actions {
		actions[verb] = r.spans(ran)
	}
	return map[string]any{"start": r.spans(r.start), "actions": actions}
}

func played(modules []*Module) (map[string]any, error) {
	diagram, ok := os.LookupEnv("WEFT_DIAGRAM")
	if !ok {
		return nil, errors.New("WEFT_DIAGRAM is not set")
	}
	name := adapter
	if given, ok := os.LookupEnv("WEFT_ADAPTER"); ok {
		name = given
	}
	said, err := ask(false, []byte(diagram))
	if err != nil {
		return nil, err
	}
	var code []block
	if err := json.Unmarshal(said, &code); err != nil {
		return nil, err
	}
	r, err := newRunner(code, name, modules)
	if err != nil {
		return nil, err
	}
	keys := []string{}
	for _, s := range states {
		keys = append(keys, s.key)
	}
	var report any = map[string]any{"uses": r.hello, "serves": r.serves, "states": keys}
	for {
		message, err := json.Marshal(report)
		if err != nil {
			return nil, err
		}
		said, err := ask(true, message)
		if err != nil {
			return nil, err
		}
		var t turn
		if err := json.Unmarshal(said, &t); err != nil {
			return nil, err
		}
		if t.Done {
			return r.ran(), nil
		}
		if report, err = r.play(t); err != nil {
			return nil, err
		}
	}
}

func Run(modules ...*Module) int {
	if err := load(os.Getenv("WEFT_LIB")); err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 1
	}
	ran, err := played(modules)
	said := map[string]any{"ran": ran}
	if err != nil {
		said = map[string]any{"error": err.Error()}
	}
	message, _ := json.Marshal(said)
	if derr := done(message); derr != nil {
		err = errors.Join(err, derr)
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 1
	}
	return 0
}
