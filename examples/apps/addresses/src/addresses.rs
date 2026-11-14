use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::thread;

use serde::{Deserialize, Serialize};
use tiny_http::{Header, Method, Request, Response, Server};
use weft_sdk::{self as weft, Json, Module, json};

#[derive(Clone, Serialize, Deserialize)]
struct Address {
    owner: String,
    street: String,
}

static URL: OnceLock<String> = OnceLock::new();
static BOOK: Mutex<BTreeMap<String, Address>> = Mutex::new(BTreeMap::new());

fn header(request: &Request, name: &str) -> String {
    request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map_or(String::new(), |h| h.value.as_str().to_string())
}

fn handled(method: &Method, id: &str, service: &str, body: &str) -> (u16, Json) {
    let Ok(mut book) = BOOK.lock() else {
        return (503, json!({ "error": "the address book is busy" }));
    };
    match method {
        Method::Put => match serde_json::from_str::<Address>(body) {
            Ok(address) => {
                book.insert(id.to_string(), address.clone());
                (201, json!(address))
            }
            Err(e) => (400, json!({ "error": e.to_string() })),
        },
        Method::Get if service.is_empty() => (401, json!({ "error": "a service reads an address" })),
        Method::Get => match book.get(id) {
            Some(address) => (200, json!(address)),
            None => (404, json!({ "error": "no such address" })),
        },
        _ => (405, json!({ "error": "unknown method" })),
    }
}

fn served(server: Server) {
    for mut request in server.incoming_requests() {
        let id = request
            .url()
            .trim_start_matches("/addresses/")
            .to_string();
        let service = header(&request, "x-service");
        let mut body = String::new();
        let _ = request.as_reader().read_to_string(&mut body);
        let (status, said) = handled(request.method(), &id, &service, &body);
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
        .ok_or("the address book has no address")?;
    thread::spawn(move || served(server));
    Ok(URL.get_or_init(|| format!("http://{at}")).clone())
}

pub fn reset() -> Result<(), String> {
    BOOK.lock().map_err(|e| e.to_string())?.clear();
    Ok(())
}

fn book() -> BTreeMap<String, Address> {
    BOOK.lock().map(|b| b.clone()).unwrap_or_default()
}

pub fn module() -> Module {
    weft::state("addresses.book", book);
    weft::module!()
        .function("url", url)
        .function("reset", reset)
}
