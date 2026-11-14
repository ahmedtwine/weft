package authz

import (
	"bytes"
	"context"
	_ "embed"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"strings"
	"sync"
	"time"

	"github.com/ahmedtwine/weft/sdk/go"
	"github.com/testcontainers/testcontainers-go"
	"github.com/testcontainers/testcontainers-go/wait"
)

//go:embed schema.zed
var schema string

const (
	image  = "authzed/spicedb:v1.45.0"
	key    = "weft"
	prefix = "/authz/"
)

type ObjectRef struct {
	Type string `json:"type"`
	ID   string `json:"id"`
}

type SubjectRef struct {
	Object           ObjectRef `json:"object"`
	OptionalRelation string    `json:"optionalRelation,omitempty"`
}

type Relationship struct {
	Resource ObjectRef  `json:"resource"`
	Relation string     `json:"relation"`
	Subject  SubjectRef `json:"subject"`
}

type Update struct {
	Operation    string       `json:"operation"`
	Relationship Relationship `json:"relationship"`
}

var (
	mu       sync.Mutex
	spicedb  string
	config   = map[string]map[string]string{}
	checks   []string
	listener net.Listener
)

func spice(path string, body any) (map[string]any, error) {
	data, err := json.Marshal(body)
	if err != nil {
		return nil, err
	}
	req, err := http.NewRequest(http.MethodPost, spicedb+path, bytes.NewReader(data))
	if err != nil {
		return nil, err
	}
	req.Header.Set("authorization", "Bearer "+key)
	req.Header.Set("content-type", "application/json")
	res, err := http.DefaultClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer res.Body.Close()
	said, _ := io.ReadAll(res.Body)
	if res.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("spicedb %s: %d %s", path, res.StatusCode, said)
	}
	out := map[string]any{}
	return out, json.Unmarshal(said, &out)
}

func started() error {
	if spicedb != "" {
		return nil
	}
	ctx := context.Background()
	ctr, err := testcontainers.Run(ctx, image,
		testcontainers.WithCmd("serve", "--grpc-preshared-key", key, "--datastore-engine", "memory",
			"--http-enabled", "--skip-release-check", "--log-level", "warn"),
		testcontainers.WithExposedPorts("8443/tcp"),
		testcontainers.WithWaitStrategy(wait.ForHTTP("/healthz").WithPort("8443/tcp").WithStartupTimeout(3*time.Minute)),
	)
	if err != nil {
		return err
	}
	endpoint, err := ctr.PortEndpoint(ctx, "8443/tcp", "http")
	if err != nil {
		return err
	}
	spicedb = endpoint
	_, err = spice("/v1/schema/write", map[string]string{"schema": schema})
	return err
}

func check(resource ObjectRef, permission string, subject SubjectRef) (bool, error) {
	said, err := spice("/v1/permissions/check", map[string]any{
		"consistency": map[string]bool{"fullyConsistent": true},
		"resource":    map[string]string{"objectType": resource.Type, "objectId": resource.ID},
		"permission":  permission,
		"subject": map[string]any{
			"object":           map[string]string{"objectType": subject.Object.Type, "objectId": subject.Object.ID},
			"optionalRelation": subject.OptionalRelation,
		},
	})
	if err != nil {
		return false, err
	}
	allowed := said["permissionship"] == "PERMISSIONSHIP_HAS_PERMISSION"
	checks = append(checks, fmt.Sprintf("%s:%s#%s@%s:%s=%t", resource.Type, resource.ID, permission, subject.Object.Type, subject.Object.ID, allowed))
	return allowed, nil
}

func caller(r *http.Request) (SubjectRef, error) {
	given := strings.TrimPrefix(r.Header.Get("authorization"), "Bearer ")
	kind, id, ok := strings.Cut(given, ":")
	if !ok {
		return SubjectRef{}, errors.New("unauthenticated")
	}
	return SubjectRef{Object: ObjectRef{Type: kind, ID: id}}, nil
}

func answer(w http.ResponseWriter, status int, body any) {
	w.Header().Set("content-type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(body)
}

func handle(w http.ResponseWriter, r *http.Request) {
	mu.Lock()
	defer mu.Unlock()
	who, err := caller(r)
	if err != nil {
		answer(w, http.StatusUnauthorized, map[string]string{"code": "unauthenticated"})
		return
	}
	var req struct {
		Resource   ObjectRef  `json:"resource"`
		Permission string     `json:"permission"`
		Subject    SubjectRef `json:"subject"`
		Object     ObjectRef  `json:"object"`
		Key        string     `json:"key"`
		Keys       []string   `json:"keys"`
		Value      string     `json:"value"`
		Updates    []Update   `json:"updates"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		answer(w, http.StatusBadRequest, map[string]string{"code": "invalid_argument"})
		return
	}
	gate := func(object ObjectRef, permission string) bool {
		allowed, err := check(object, permission, who)
		if err != nil {
			answer(w, http.StatusServiceUnavailable, map[string]string{"code": "unavailable", "message": err.Error()})
			return false
		}
		if !allowed {
			answer(w, http.StatusForbidden, map[string]string{"code": "permission_denied", "message": "denied"})
		}
		return allowed
	}
	switch strings.TrimPrefix(r.URL.Path, prefix) {
	case "Check":
		subject := req.Subject
		if subject.Object.Type == "" {
			subject = who
		}
		allowed, err := check(req.Resource, req.Permission, subject)
		if err != nil {
			answer(w, http.StatusServiceUnavailable, map[string]string{"code": "unavailable", "message": err.Error()})
			return
		}
		answer(w, http.StatusOK, map[string]bool{"allowed": allowed})
	case "WriteRelationships":
		updates := make([]map[string]any, 0, len(req.Updates))
		for _, u := range req.Updates {
			rel := u.Relationship
			updates = append(updates, map[string]any{
				"operation": u.Operation,
				"relationship": map[string]any{
					"resource": map[string]string{"objectType": rel.Resource.Type, "objectId": rel.Resource.ID},
					"relation": rel.Relation,
					"subject": map[string]any{
						"object":           map[string]string{"objectType": rel.Subject.Object.Type, "objectId": rel.Subject.Object.ID},
						"optionalRelation": rel.Subject.OptionalRelation,
					},
				},
			})
		}
		if _, err := spice("/v1/relationships/write", map[string]any{"updates": updates}); err != nil {
			answer(w, http.StatusServiceUnavailable, map[string]string{"code": "unavailable", "message": err.Error()})
			return
		}
		answer(w, http.StatusOK, map[string]string{})
	case "GetConfig":
		if !gate(req.Object, "read") {
			return
		}
		values := map[string]string{}
		for k, v := range config[req.Object.Type+":"+req.Object.ID] {
			if len(req.Keys) == 0 || contains(req.Keys, k) {
				values[k] = v
			}
		}
		answer(w, http.StatusOK, map[string]any{"values": values})
	case "SetConfig":
		if !gate(req.Object, "manage") {
			return
		}
		if !json.Valid([]byte(req.Value)) {
			answer(w, http.StatusBadRequest, map[string]string{"code": "invalid_argument", "message": "value must be valid JSON"})
			return
		}
		object := req.Object.Type + ":" + req.Object.ID
		if config[object] == nil {
			config[object] = map[string]string{}
		}
		config[object][req.Key] = req.Value
		answer(w, http.StatusOK, map[string]string{})
	default:
		answer(w, http.StatusNotFound, map[string]string{"code": "unimplemented"})
	}
}

func contains(all []string, one string) bool {
	for _, each := range all {
		if each == one {
			return true
		}
	}
	return false
}

func URL() (string, error) {
	if listener == nil {
		l, err := net.Listen("tcp", "127.0.0.1:0")
		if err != nil {
			return "", err
		}
		listener = l
		go func() { _ = http.Serve(l, http.HandlerFunc(handle)) }()
	}
	return "http://" + listener.Addr().String() + prefix, nil
}

func Reset() error {
	mu.Lock()
	defer mu.Unlock()
	if err := started(); err != nil {
		return err
	}
	for _, kind := range []string{"tenant", "credential", "store"} {
		if _, err := spice("/v1/relationships/delete", map[string]any{"relationshipFilter": map[string]string{"resourceType": kind}}); err != nil {
			return err
		}
	}
	config = map[string]map[string]string{}
	checks = nil
	return nil
}

type State struct {
	Checks []string `json:"checks"`
}

func Get() State {
	mu.Lock()
	defer mu.Unlock()
	now := State{Checks: append([]string{}, checks...)}
	checks = nil
	return now
}

func Module() *weft.Module {
	return weft.NewModule().
		Schema("authzSchema", `{"type":"object","properties":{"checks":{"type":"array","items":{"type":"string"},"weft":{"read":"authz.checks"}}},"required":["checks"]}`).
		Function("url", URL).
		Function("reset", Reset).
		Function("authz", Get)
}
