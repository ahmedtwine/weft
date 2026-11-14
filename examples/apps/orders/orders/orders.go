package orders

import (
	"context"
	"encoding/json"
	"net"
	"net/http"
	"strings"
	"sync"

	"github.com/ahmedtwine/weft/sdk/go"
	"github.com/redis/go-redis/v9"
	"github.com/testcontainers/testcontainers-go"
	"github.com/testcontainers/testcontainers-go/wait"
)

const image = "redis:7-alpine"

type Order struct {
	Customer  string `json:"customer"`
	AddressID string `json:"address_id"`
}

var (
	mu       sync.Mutex
	db       *redis.Client
	listener net.Listener
)

func started(ctx context.Context) error {
	if db != nil {
		return nil
	}
	ctr, err := testcontainers.Run(ctx, image,
		testcontainers.WithExposedPorts("6379/tcp"),
		testcontainers.WithWaitStrategy(wait.ForListeningPort("6379/tcp")),
	)
	if err != nil {
		return err
	}
	endpoint, err := ctr.PortEndpoint(ctx, "6379/tcp", "")
	if err != nil {
		return err
	}
	db = redis.NewClient(&redis.Options{Addr: endpoint})
	return db.Ping(ctx).Err()
}

func reply(w http.ResponseWriter, status int, body any) {
	w.Header().Set("content-type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(body)
}

func order(ctx context.Context, id string) (Order, bool) {
	fields := db.HGetAll(ctx, "order:"+id).Val()
	return Order{Customer: fields["customer"], AddressID: fields["address_id"]}, len(fields) > 0
}

func handle(w http.ResponseWriter, r *http.Request) {
	mu.Lock()
	defer mu.Unlock()
	ctx := r.Context()
	id := strings.TrimPrefix(r.URL.Path, "/orders/")
	found, known := order(ctx, id)
	switch r.Method {
	case http.MethodPut:
		var given Order
		if err := json.NewDecoder(r.Body).Decode(&given); err != nil {
			reply(w, http.StatusBadRequest, map[string]string{"error": err.Error()})
			return
		}
		db.HSet(ctx, "order:"+id, "customer", given.Customer, "address_id", given.AddressID)
		reply(w, http.StatusCreated, given)
	case http.MethodPatch:
		var change struct {
			AddressID string `json:"address_id"`
		}
		if err := json.NewDecoder(r.Body).Decode(&change); err != nil {
			reply(w, http.StatusBadRequest, map[string]string{"error": err.Error()})
			return
		}
		if !known || found.Customer != r.Header.Get("x-customer") {
			reply(w, http.StatusForbidden, map[string]string{"error": "the order belongs to another customer"})
			return
		}
		db.HSet(ctx, "order:"+id, "address_id", change.AddressID)
		found.AddressID = change.AddressID
		reply(w, http.StatusOK, found)
	case http.MethodGet:
		if !known {
			reply(w, http.StatusNotFound, map[string]string{"error": "no such order"})
			return
		}
		reply(w, http.StatusOK, found)
	default:
		reply(w, http.StatusMethodNotAllowed, map[string]string{"error": r.Method})
	}
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
	return "http://" + listener.Addr().String(), nil
}

func Reset() error {
	mu.Lock()
	defer mu.Unlock()
	ctx := context.Background()
	if err := started(ctx); err != nil {
		return err
	}
	return db.FlushAll(ctx).Err()
}

func Module() *weft.Module {
	weft.State("orders.orders", func() map[string]Order {
		mu.Lock()
		defer mu.Unlock()
		ctx := context.Background()
		all := map[string]Order{}
		for _, key := range db.Keys(ctx, "order:*").Val() {
			id := strings.TrimPrefix(key, "order:")
			all[id], _ = order(ctx, id)
		}
		return all
	})
	return weft.NewModule().
		Function("url", URL).
		Function("reset", Reset)
}
