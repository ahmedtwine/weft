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
	"strings"
	"testing"
)

const (
	adapter = "go"
	start   = "start"
	parent  = "--parent"
)

var errType = reflect.TypeFor[error]()

type function struct {
	id    string
	value reflect.Value
}

func identify(f any) (function, error) {
	value := reflect.ValueOf(f)
	if value.Kind() != reflect.Func || value.Type().NumIn() > 1 || value.Type().NumOut() > 2 {
		return function{}, fmt.Errorf("weft: %T is a func with at most one input and two results", f)
	}
	name := runtime.FuncForPC(value.Pointer()).Name()
	return function{id: name[strings.LastIndex(name, "/")+1:], value: value}, nil
}

type module struct {
	Path    string            `json:"path"`
	Reset   *string           `json:"reset"`
	State   *string           `json:"state"`
	URL     *string           `json:"url"`
	Actions map[string]string `json:"actions"`
}

type block struct {
	Adapter string            `json:"adapter"`
	Modules map[string]module `json:"modules"`
}

type used struct {
	file  string
	name  string
	value reflect.Value
}

type read struct {
	object string
	value  reflect.Value
}

type covered map[string]map[string]bool

type runner struct {
	calls   map[string]used
	states  []read
	serves  any
	started covered
	ran     map[string]covered
}

type failure struct {
	message string
}

func place(at string) string {
	path, _ := filepath.Abs(at)
	if found, err := filepath.EvalSymlinks(path); err == nil {
		return found
	}
	return path
}

func newRunner(code []block, name string, functions []any) (*runner, error) {
	known := map[string]reflect.Value{}
	for _, f := range functions {
		found, err := identify(f)
		if err != nil {
			return nil, err
		}
		known[found.id] = found.value
	}
	find := func(object, id string) (reflect.Value, error) {
		if value, ok := known[id]; ok {
			return value, nil
		}
		return reflect.Value{}, fmt.Errorf("`module %q` maps `%s`, and weft.Run of the runner has %s", object, id, strings.Join(slices.Sorted(maps.Keys(known)), ", "))
	}
	r := &runner{calls: map[string]used{}, started: covered{}, ran: map[string]covered{}}
	for _, b := range code {
		if b.Adapter != name {
			continue
		}
		for _, object := range slices.Sorted(maps.Keys(b.Modules)) {
			m := b.Modules[object]
			file := place(m.Path)
			ids := slices.Sorted(maps.Values(m.Actions))
			if m.Reset != nil {
				ids = append(ids, *m.Reset)
			}
			for _, id := range ids {
				value, err := find(object, id)
				if err != nil {
					return nil, err
				}
				r.calls[id] = used{file: file, name: id[strings.LastIndex(id, ".")+1:], value: value}
			}
			if m.State != nil {
				value, err := find(object, *m.State)
				if err != nil {
					return nil, err
				}
				r.states = append(r.states, read{object: object, value: value})
			}
			if m.URL != nil {
				value, err := find(object, *m.URL)
				if err != nil {
					return nil, err
				}
				result, fail := called(value, nil)
				if fail != nil {
					return nil, errors.New(fail.message)
				}
				r.serves = fmt.Sprint(result)
			}
		}
	}
	return r, nil
}

func (r *runner) observed() (map[string]any, *failure) {
	result := map[string]any{}
	for _, s := range r.states {
		found, fail := called(s.value, nil)
		if fail != nil {
			return nil, fail
		}
		result[s.object] = found
	}
	return result, nil
}

func called(function reflect.Value, input json.RawMessage) (result any, fail *failure) {
	defer func() {
		if p := recover(); p != nil {
			fail = &failure{message: fmt.Sprintf("panic: %v%s", p, panicked())}
		}
	}()
	kind := function.Type()
	var args []reflect.Value
	for in := range kind.Ins() {
		value := reflect.New(in)
		if len(input) > 0 && string(input) != "null" {
			if err := json.Unmarshal(input, value.Interface()); err != nil {
				return nil, &failure{message: fmt.Sprintf("the input %s is no value of %s: %v", input, in, err)}
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
	Function string          `json:"function"`
	Input    json.RawMessage `json:"input"`
}

type turn struct {
	Step  *string `json:"step"`
	Calls []call  `json:"calls"`
	Done  bool    `json:"done"`
}

func (r *runner) play(t turn) any {
	ran := covered{}
	switch {
	case t.Step == nil:
	case *t.Step == start:
		ran = r.started
	default:
		if r.ran[*t.Step] == nil {
			r.ran[*t.Step] = covered{}
		}
		ran = r.ran[*t.Step]
	}
	var result any
	for _, c := range t.Calls {
		u, ok := r.calls[c.Function]
		if !ok {
			return map[string]string{"thrown": fmt.Sprintf("the runner has no function `%s`", c.Function)}
		}
		if ran[u.file] == nil {
			ran[u.file] = map[string]bool{}
		}
		ran[u.file][u.name] = true
		found, fail := called(u.value, c.Input)
		if fail != nil {
			return map[string]string{"thrown": fail.message}
		}
		result = found
	}
	states, fail := r.observed()
	if fail != nil {
		return map[string]string{"thrown": fail.message}
	}
	return map[string]any{"states": states, "result": result}
}

type span struct {
	File string `json:"file"`
	From int    `json:"from"`
	To   int    `json:"to"`
}

func spans(ran covered) []span {
	cwd, _ := os.Getwd()
	cwd = place(cwd)
	out := []span{}
	for file, names := range ran {
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

func (r *runner) covered() map[string]any {
	actions := map[string][]span{}
	for verb, ran := range r.ran {
		actions[verb] = spans(ran)
	}
	return map[string]any{"start": spans(r.started), "actions": actions}
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

func played(s setup, functions []any) (map[string]any, error) {
	said, err := ask(join, []byte(s.Diagram))
	if err != nil {
		return nil, err
	}
	var code []block
	if err := json.Unmarshal(said, &code); err != nil {
		return nil, err
	}
	r, err := newRunner(code, s.Adapter, functions)
	if err != nil {
		return nil, err
	}
	var report any = map[string]any{"adapter": s.Adapter, "serves": r.serves}
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
			return r.covered(), nil
		}
		report = r.play(t)
	}
}

func Run(functions ...any) int {
	s, err := env()
	if err == nil {
		err = load(s.Lib)
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 1
	}
	ran, err := played(s, functions)
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

func Test(t *testing.T, file string, functions ...any) {
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
		Run(functions...)
	}
	rest, _ := io.ReadAll(output)
	report := first + string(rest)
	if err := command.Wait(); err != nil {
		t.Fatalf("%v\n%s", err, report)
	}
	t.Log(report)
}
