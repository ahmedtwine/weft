package weft

import (
	"bufio"
	"encoding/json"
	"errors"
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"io"
	"maps"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"runtime"
	"slices"
	"strconv"
	"strings"
	"testing"
)

const (
	adapter = "go"
	start   = "start"
	parent  = "--parent"
)

type TestCase struct{}

type Generator[T any] struct {
	draw func() ([]byte, error)
}

type Pool[T any] struct {
	name string
}

var testCase = reflect.TypeFor[TestCase]()

func drawn[T any](said []byte, err error) T {
	var value T
	if err == nil {
		err = json.Unmarshal(said, &value)
	}
	if err != nil {
		panic(err)
	}
	return value
}

func Draw[T any](tc TestCase, generator Generator[T]) T {
	return drawn[T](generator.draw())
}

func Schema[T any]() Generator[T] {
	return Generator[T]{draw: func() ([]byte, error) {
		schema, err := json.Marshal(schemaOf(reflect.TypeFor[T](), map[reflect.Type]bool{}))
		if err != nil {
			return nil, err
		}
		return ask(generate, schema)
	}}
}

func (tc TestCase) Assume(condition bool) {
	if !condition {
		panic(stopped("assume"))
	}
}

func (tc TestCase) Target(score float64, label string) {
	if err := target(score, label); err != nil {
		panic(err)
	}
}

func NewPool[T any](tc TestCase, name string) *Pool[T] {
	return &Pool[T]{name: name}
}

func (p *Pool[T]) Add(tc TestCase, value T) {
	said, err := json.Marshal(value)
	if err == nil {
		err = poolAdd(p.name, said)
	}
	if err != nil {
		panic(err)
	}
}

func (p *Pool[T]) values(consume bool) Generator[T] {
	return Generator[T]{draw: func() ([]byte, error) { return poolGenerate(p.name, consume) }}
}

func (p *Pool[T]) ValuesReusable() Generator[T] {
	return p.values(false)
}

func (p *Pool[T]) ValuesConsumed() Generator[T] {
	return p.values(true)
}

type export struct {
	name     string
	function reflect.Value
}

type Module struct {
	file    string
	exports []export
}

func NewModule() *Module {
	_, file, _, _ := runtime.Caller(1)
	if found, err := filepath.EvalSymlinks(file); err == nil {
		file = found
	}
	return &Module{file: file}
}

func (m *Module) Function(name string, function any) *Module {
	value := reflect.ValueOf(function)
	if value.Kind() != reflect.Func || len(inputs(value.Type())) > 1 || value.Type().NumOut() > 2 {
		panic(fmt.Sprintf("weft: `%s` is a func with at most one input, a TestCase and two results", name))
	}
	m.exports = slices.DeleteFunc(m.exports, func(e export) bool { return e.name == name })
	m.exports = append(m.exports, export{name: name, function: value})
	return m
}

type used struct {
	function reflect.Value
	module   int
	name     string
}

type read struct {
	key    string
	file   string
	schema map[string]any
	read   func() any
}

var states []read

func State[T any](key string, get func() T) {
	_, file, _, _ := runtime.Caller(1)
	if found, err := filepath.EvalSymlinks(file); err == nil {
		file = found
	}
	states = slices.DeleteFunc(states, func(r read) bool { return r.key == key })
	states = append(states, read{key: key, file: file, schema: schemaOf(reflect.TypeFor[T](), map[reflect.Type]bool{}), read: func() any { return get() }})
}

func schemaOf(t reflect.Type, seen map[reflect.Type]bool) map[string]any {
	for t.Kind() == reflect.Pointer {
		t = t.Elem()
	}
	switch t.Kind() {
	case reflect.String:
		return map[string]any{"type": "string"}
	case reflect.Bool:
		return map[string]any{"type": "boolean"}
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64,
		reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64:
		return map[string]any{"type": "integer"}
	case reflect.Float32, reflect.Float64:
		return map[string]any{"type": "number"}
	case reflect.Slice, reflect.Array:
		if t.Elem().Kind() == reflect.Uint8 {
			return map[string]any{"type": "string"}
		}
		return map[string]any{"type": "array", "items": schemaOf(t.Elem(), seen)}
	case reflect.Map:
		return map[string]any{"type": "object", "additionalProperties": schemaOf(t.Elem(), seen)}
	case reflect.Struct:
		out := map[string]any{"type": "object", "title": t.Name()}
		if seen[t] {
			return out
		}
		seen[t] = true
		properties := map[string]any{}
		required := []string{}
		for i := range t.NumField() {
			f := t.Field(i)
			name, _, _ := strings.Cut(f.Tag.Get("json"), ",")
			if !f.IsExported() || name == "-" {
				continue
			}
			if name == "" {
				name = f.Name
			}
			field := schemaOf(f.Type, seen)
			for _, bound := range []string{"minimum", "maximum"} {
				if given, ok := f.Tag.Lookup(bound); ok {
					if n, err := strconv.ParseFloat(given, 64); err == nil {
						field[bound] = n
					}
				}
			}
			properties[name] = field
			required = append(required, name)
		}
		out["properties"] = properties
		out["required"] = required
		out["additionalProperties"] = false
		return out
	}
	return map[string]any{}
}

func (r *runner) observed() (result map[string]any, fault string) {
	defer func() {
		if recover() != nil {
			fault = panicked()
		}
	}()
	result = map[string]any{}
	for _, s := range r.states {
		result[s.key] = s.read()
	}
	return result, ""
}

type runner struct {
	states      []read
	serves      string
	modules     []*Module
	reset       []*used
	actions     []*used
	saidReset   []any
	saidActions []any
	started     map[int]map[string]bool
	covered     map[string]map[int]map[string]bool
}

type block struct {
	Adapter string            `json:"adapter"`
	Modules map[string]string `json:"modules"`
}

type failure struct {
	refused string
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

func inputs(kind reflect.Type) []reflect.Type {
	return slices.DeleteFunc(slices.Collect(kind.Ins()), func(in reflect.Type) bool { return in == testCase })
}

func input(function reflect.Value) any {
	for _, in := range inputs(function.Type()) {
		return schemaOf(in, map[reflect.Type]bool{})
	}
	return nil
}

func newRunner(code []block, name string, modules []*Module) (*runner, error) {
	r := &runner{modules: modules, started: map[int]map[string]bool{}, covered: map[string]map[int]map[string]bool{}}
	for _, b := range code {
		if b.Adapter != name {
			continue
		}
		names := slices.Sorted(maps.Keys(b.Modules))
		for _, module := range names {
			at := b.Modules[module]
			index := slices.IndexFunc(modules, func(m *Module) bool { return m.file == place(at) })
			if index < 0 {
				return nil, fmt.Errorf("the runner has no module `%s`", at)
			}
			for _, s := range states {
				if s.file == modules[index].file {
					r.states = append(r.states, s)
				}
			}
			for _, e := range modules[index].exports {
				u := &used{function: e.function, module: index, name: e.name}
				said := map[string]any{"function": []string{module, e.name}, "input": input(e.function)}
				switch e.name {
				case "url":
					result, fail := called(e.function, nil)
					if fail != nil {
						return nil, errors.New(fail.message)
					}
					r.serves = fmt.Sprint(result)
				case "reset":
					r.reset = append(r.reset, u)
					r.saidReset = append(r.saidReset, said)
				default:
					r.actions = append(r.actions, u)
					r.saidActions = append(r.saidActions, said)
				}
			}
		}
	}
	return r, nil
}

func called(function reflect.Value, input json.RawMessage) (result any, fail *failure) {
	defer func() {
		p := recover()
		if cause, stops := p.(stopped); stops {
			fail = &failure{refused: string(cause)}
		} else if p != nil {
			fail = &failure{fault: true, message: fmt.Sprintf("panic: %v%s", p, panicked())}
		}
	}()
	kind := function.Type()
	var args []reflect.Value
	for in := range kind.Ins() {
		value := reflect.New(in)
		if in != testCase && len(input) > 0 && string(input) != "null" {
			if err := json.Unmarshal(input, value.Interface()); err != nil {
				return nil, &failure{refused: "schema"}
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
	list := r.actions
	switch {
	case t.Step == nil:
	case *t.Step == start:
		ran, list = r.started, r.reset
	default:
		if r.covered[*t.Step] == nil {
			r.covered[*t.Step] = map[int]map[string]bool{}
		}
		ran = r.covered[*t.Step]
	}
	for _, c := range t.Calls {
		if c.Use >= len(list) {
			return nil, fmt.Errorf("the runner has no function %d", c.Use)
		}
		u := list[c.Use]
		r.note(ran, u)
		_, fail := called(u.function, c.Input)
		switch {
		case fail == nil:
		case fail.refused != "":
			return map[string]string{"refused": fail.refused}, nil
		default:
			return map[string]string{"thrown": fail.message}, nil
		}
	}
	found, fault := r.observed()
	if fault != "" {
		return map[string]string{"thrown": fault}, nil
	}
	return map[string]any{"states": found}, nil
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
	for verb, ran := range r.covered {
		actions[verb] = r.spans(ran)
	}
	return map[string]any{"start": r.spans(r.started), "actions": actions}
}

type setup struct {
	Diagram string `json:"diagram"`
	Lib     string `json:"lib"`
	Adapter string `json:"adapter"`
}

func env() (setup, error) {
	var s setup
	text, ok := os.LookupEnv("WEFT")
	if !ok {
		return s, errors.New("WEFT is not set")
	}
	err := json.Unmarshal([]byte(text), &s)
	return s, err
}

func played(s setup, modules []*Module) (map[string]any, error) {
	said, err := ask(join, []byte(s.Diagram))
	if err != nil {
		return nil, err
	}
	var code []block
	if err := json.Unmarshal(said, &code); err != nil {
		return nil, err
	}
	r, err := newRunner(code, s.Adapter, modules)
	if err != nil {
		return nil, err
	}
	keys := map[string]any{}
	for _, s := range r.states {
		keys[s.key] = s.schema
	}
	var report any = map[string]any{"adapter": s.Adapter, "start": r.saidReset, "actions": r.saidActions, "serves": r.serves, "states": keys}
	for {
		message, err := json.Marshal(report)
		if err != nil {
			return nil, err
		}
		said, err := ask(turns, message)
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
	s, err := env()
	if err == nil {
		err = load(s.Lib)
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 1
	}
	ran, err := played(s, modules)
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

func Test(t *testing.T, file string, modules ...*Module) {
	weft := os.Getenv("WEFT_BIN")
	if weft == "" {
		weft = "weft"
	}
	command := exec.Command(weft, parent, adapter, file)
	command.Stderr = os.Stderr
	stdin, err := command.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	defer stdin.Close()
	stdout, err := command.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	if err := command.Start(); err != nil {
		t.Fatal(err)
	}
	output := bufio.NewReader(stdout)
	first, _ := output.ReadString('\n')
	var given struct {
		Cwd string            `json:"cwd"`
		Env map[string]string `json:"env"`
	}
	joins := json.Unmarshal([]byte(first), &given) == nil
	if joins {
		first = ""
		if err := os.Chdir(given.Cwd); err != nil {
			t.Fatal(err)
		}
		for key, value := range given.Env {
			os.Setenv(key, value)
		}
		Run(modules...)
	}
	rest, _ := io.ReadAll(output)
	report := first + string(rest)
	if err := command.Wait(); err != nil {
		t.Fatalf("%v\n%s", err, report)
	}
	t.Log(report)
}
