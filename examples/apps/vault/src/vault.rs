use std::collections::HashMap;
use std::env;
use std::sync::{Mutex, OnceLock};
use std::thread;

use tiny_http::{Header, Method, Request, Response, Server};
use weft_sdk::{Json, Module, json};

const SERVICE: &str = "service:vault";

static URL: OnceLock<String> = OnceLock::new();
static STORE: Mutex<Option<HashMap<String, Json>>> = Mutex::new(None);
static READS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn authz(rpc: &str, token: &str, body: Json) -> Result<Json, String> {
    let base = env::var("WEFT_AUTHZ_URL").map_err(|_| "WEFT_AUTHZ_URL is not set".to_string())?;
    let mut response = ureq::post(format!("{base}/{rpc}"))
        .config()
        .http_status_as_error(false)
        .build()
        .header("authorization", format!("Bearer {token}"))
        .send_json(body)
        .map_err(|e| format!("authz {rpc}: {e}"))?;
    let status = response.status().as_u16();
    let said: Json = response.body_mut().read_json().map_err(|e| e.to_string())?;
    match status {
        200 => Ok(said),
        _ => Err(format!("authz {rpc}: {status} {said}")),
    }
}

fn object(kind: &str, id: &str) -> Json {
    json!({ "type": kind, "id": id })
}

fn subject(token: &str) -> Json {
    let (kind, id) = token.split_once(':').unwrap_or(("", token));
    json!({ "object": object(kind, id) })
}

fn allowed(token: &str, resource: Json, permission: &str) -> Result<bool, String> {
    let said = authz(
        "Check",
        token,
        json!({ "resource": resource, "permission": permission, "subject": subject(token) }),
    )?;
    Ok(said["allowed"] == true)
}

fn touch(resource: Json, relation: &str, subject: Json) -> Json {
    json!({ "operation": "OPERATION_TOUCH", "relationship": { "resource": resource, "relation": relation, "subject": subject } })
}

fn handled(method: &Method, path: &str, token: &str, owner: &str, body: &str) -> (u16, Json) {
    let Some(path) = path.strip_prefix("/v1/credentials") else {
        return (404, json!({ "code": "not_found" }));
    };
    if owner.is_empty() {
        return (400, json!({ "code": "invalid_argument", "message": "x-tenant is required" }));
    }
    let name = format!("/{owner}{path}");
    let credential = object("credential", &name);
    let refused = |e: String| (503, json!({ "code": "unavailable", "message": e }));
    match method {
        Method::Post => {
            match allowed(token, object("tenant", owner), "is_member") {
                Ok(true) => {}
                Ok(false) => return (403, json!({ "code": "permission_denied", "message": "not allowed" })),
                Err(e) => return refused(e),
            }
            let updates = json!({ "updates": [
                touch(credential.clone(), "tenant", json!({ "object": object("tenant", owner) })),
                touch(credential, "owner", subject(token)),
            ]});
            if let Err(e) = authz("WriteRelationships", SERVICE, updates) {
                return refused(e);
            }
            let value: Json = serde_value(body);
            if let Ok(mut store) = STORE.lock() {
                store.get_or_insert_default().insert(name.clone(), value);
            }
            (201, json!({ "path": path, "version": "1" }))
        }
        Method::Get => {
            match allowed(token, credential, "read") {
                Ok(true) => {}
                Ok(false) => return (403, json!({ "code": "permission_denied", "message": "not allowed" })),
                Err(e) => return refused(e),
            }
            let value = STORE
                .lock()
                .ok()
                .and_then(|store| store.as_ref()?.get(&name).cloned());
            match value {
                Some(value) => {
                    if let Ok(mut reads) = READS.lock() {
                        reads.push(owner.to_string());
                    }
                    (200, json!({ "path": path, "version": "1", "value": value }))
                }
                None => (404, json!({ "code": "not_found", "message": "does not exist" })),
            }
        }
        _ => (405, json!({ "code": "invalid_argument" })),
    }
}

fn serde_value(body: &str) -> Json {
    body.parse::<Json>()
        .map_or(Json::Null, |given| given["value"].clone())
}

fn header(request: &Request, name: &str) -> String {
    request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map_or(String::new(), |h| h.value.as_str().to_string())
}

fn served(server: Server) {
    for mut request in server.incoming_requests() {
        let token = header(&request, "authorization")
            .trim_start_matches("Bearer ")
            .to_string();
        let owner = header(&request, "x-tenant");
        let mut body = String::new();
        let _ = request.as_reader().read_to_string(&mut body);
        let (status, said) = match token.is_empty() {
            true => (401, json!({ "code": "unauthenticated" })),
            false => handled(request.method(), request.url(), &token, &owner, &body),
        };
        let content = Header::from_bytes("content-type", "application/json").expect("header");
        let _ = request.respond(
            Response::from_string(said.to_string())
                .with_status_code(status)
                .with_header(content),
        );
    }
}

pub fn url() -> Result<String, String> {
    if let Some(url) = URL.get() {
        return Ok(url.clone());
    }
    let server = Server::http("127.0.0.1:0").map_err(|e| e.to_string())?;
    let at = server
        .server_addr()
        .to_ip()
        .ok_or("the vault has no address")?;
    thread::spawn(move || served(server));
    Ok(URL.get_or_init(|| format!("http://{at}")).clone())
}

pub fn reset() -> Result<(), String> {
    *STORE.lock().map_err(|e| e.to_string())? = Some(HashMap::new());
    READS.lock().map_err(|e| e.to_string())?.clear();
    Ok(())
}

pub fn state() -> Result<Json, String> {
    let mut reads = READS.lock().map_err(|e| e.to_string())?;
    let owner = reads.last().cloned().unwrap_or_default();
    reads.clear();
    Ok(json!({ "read_owner": owner }))
}

pub fn module() -> Module {
    weft_sdk::module!()
        .schema(
            "vaultSchema",
            json!({
                "type": "object",
                "properties": { "read_owner": { "type": "string", "weft": { "read": "vault.read_owner" } } },
                "required": ["read_owner"]
            }),
        )
        .function("url", url)
        .function("reset", reset)
        .function("vault", state)
}
