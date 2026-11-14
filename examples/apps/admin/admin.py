import json
import os
import urllib.error
import urllib.request

SERVICE = "service:admin"
TOOL = "service:tool"
TENANTS = [
    {"tenant": "acme", "admin": "acme-admin", "analyst": "acme-analyst", "store": "acme-store"},
    {"tenant": "globex", "admin": "globex-admin", "analyst": "globex-analyst", "store": "globex-store"},
]

state = {"credential_ref": ""}

setCredentialSchema = {
    "type": "object",
    "properties": {
        "credential_ref": {"type": "string", "enum": ["/acme/warehouse", "/globex/warehouse"]},
    },
    "required": ["credential_ref"],
    "additionalProperties": False,
    "weft": {"verb": "set_credential"},
}
stateSchema = {
    "type": "object",
    "properties": {"credential_ref": {"type": "string", "weft": {"read": "admin.credential_ref"}}},
    "required": ["credential_ref"],
}


def call(base, path, token, body, headers=None):
    request = urllib.request.Request(
        os.environ[base] + path,
        data=json.dumps(body).encode(),
        method="POST",
        headers={"authorization": f"Bearer {token}", "content-type": "application/json", **(headers or {})},
    )
    try:
        with urllib.request.urlopen(request) as response:
            return json.loads(response.read() or b"{}")
    except urllib.error.HTTPError as error:
        raise RuntimeError(f"{path}: {error.code} {error.read().decode()}") from error


def ref(kind, id):
    return {"type": kind, "id": id}


def touch(resource, relation, subject, optional=""):
    kind, id = subject.split(":", 1)
    return {
        "operation": "OPERATION_TOUCH",
        "relationship": {
            "resource": resource,
            "relation": relation,
            "subject": {"object": ref(kind, id), "optionalRelation": optional},
        },
    }


def authz(rpc, token, body):
    return call("WEFT_AUTHZ_URL", f"/{rpc}", token, body)


def add_store(t):
    tenant, store = t["tenant"], t["store"]
    authz("WriteRelationships", SERVICE, {"updates": [
        touch(ref("tenant", tenant), "admin", f"user:{t['admin']}"),
        touch(ref("tenant", tenant), "member", f"user:{t['analyst']}"),
    ]})
    call(
        "WEFT_VAULT_URL",
        "/v1/credentials/warehouse",
        f"user:{t['admin']}",
        {"value": {"tenant": tenant, "host": f"{tenant}.warehouse.example", "key": f"key-{tenant}"}},
        {"x-tenant": tenant},
    )
    authz("WriteRelationships", SERVICE, {"updates": [
        touch(ref("store", store), "tenant", f"tenant:{tenant}"),
        touch(ref("store", store), "querier", f"tenant:{tenant}", "member"),
        touch(ref("credential", f"/{tenant}/warehouse"), "reader", TOOL),
    ]})
    authz("SetConfig", f"user:{t['admin']}", {
        "object": ref("store", store),
        "key": "credential_ref",
        "value": json.dumps(f"/{tenant}/warehouse"),
    })


def provision():
    for tenant in TENANTS:
        add_store(tenant)
    state["credential_ref"] = "/acme/warehouse"


def set_credential(input):
    authz("SetConfig", "user:acme-admin", {
        "object": ref("store", "acme-store"),
        "key": "credential_ref",
        "value": json.dumps(input["credential_ref"]),
    })
    state["credential_ref"] = input["credential_ref"]


def read():
    return dict(state)
