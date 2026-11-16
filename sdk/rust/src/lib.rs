use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt::Display;
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde::de::DeserializeOwned;
pub use serde_json::{Value as Json, json};
use syn::spanned::Spanned;
use weft_wire::Given;
use weft_wire::client::said;
use weft_wire::{DONE, JOIN, TURN};

const ADAPTER: &str = "rust";
const START: &str = "start";
const WEFT: &str = "WEFT_BIN";
const PARENT: &str = "--parent";

type Call = Arc<dyn Fn(Json) -> Result<Json, String> + Send + Sync>;

pub struct Function {
    name: String,
    call: Call,
}

pub trait Callable<M>: Send + Sync + 'static {
    fn call(&self, input: Json) -> Result<Json, String>;
}

fn returned<T: Serialize, E: Display>(result: Result<T, E>) -> Result<Json, String> {
    result
        .map_err(|e| e.to_string())
        .and_then(|value| serde_json::to_value(value).map_err(|e| e.to_string()))
}

impl<F, T, E> Callable<()> for F
where
    F: Fn() -> Result<T, E> + Send + Sync + 'static,
    T: Serialize,
    E: Display,
{
    fn call(&self, _: Json) -> Result<Json, String> {
        returned(self())
    }
}

impl<F, I, T, E> Callable<(I,)> for F
where
    F: Fn(I) -> Result<T, E> + Send + Sync + 'static,
    I: DeserializeOwned,
    T: Serialize,
    E: Display,
{
    fn call(&self, input: Json) -> Result<Json, String> {
        let given = serde_json::from_value(input.clone())
            .map_err(|e| format!("the input `{input}` is no value of the parameter: {e}"))?;
        returned(self(given))
    }
}

impl Function {
    pub fn new<M>(path: &str, function: impl Callable<M>) -> Function {
        Function {
            name: path.split_whitespace().collect(),
            call: Arc::new(move |input| function.call(input)),
        }
    }
}

#[macro_export]
macro_rules! functions {
    ($($f:path),* $(,)?) => {
        vec![$($crate::Function::new(stringify!($f), $f)),*]
    };
}

struct Used {
    file: PathBuf,
    name: String,
    call: Call,
}

struct Read {
    object: String,
    call: Call,
}

type Ran = BTreeSet<(PathBuf, String)>;

struct Runner {
    calls: BTreeMap<String, Used>,
    states: Vec<Read>,
    hello: Json,
    started: Ran,
    ran: BTreeMap<String, Ran>,
}

static FAULT: Mutex<Option<String>> = Mutex::new(None);

fn place(at: &str) -> PathBuf {
    let path = env::current_dir().unwrap_or_default().join(at);
    fs::canonicalize(&path).unwrap_or(path)
}

fn called(run: &Call, input: Json) -> Result<Json, String> {
    match panic::catch_unwind(AssertUnwindSafe(|| run(input))) {
        Ok(result) => result,
        Err(payload) => Err(FAULT
            .lock()
            .ok()
            .and_then(|mut f| f.take())
            .or_else(|| payload.downcast_ref::<&str>().map(ToString::to_string))
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "a panic".into())),
    }
}

impl Runner {
    fn new(code: &Json, adapter: &str, functions: &[Function]) -> Result<Runner, String> {
        let found = |object: &str, id: &str| {
            functions
                .iter()
                .find(|f| f.name == id)
                .map(|f| f.call.clone())
                .ok_or_else(|| {
                    format!(
                        "`module \"{object}\"` maps `{id}`, and `weft::functions![…]` of the runner lists no `{id}`"
                    )
                })
        };
        let mut runner = Runner {
            calls: BTreeMap::new(),
            states: Vec::new(),
            hello: Json::Null,
            started: Ran::new(),
            ran: BTreeMap::new(),
        };
        let mut serves = None;
        let mine = code
            .as_array()
            .into_iter()
            .flatten()
            .filter(|i| i["adapter"] == adapter);
        for i in mine {
            for (object, m) in i["modules"].as_object().into_iter().flatten() {
                let file = place(m["path"].as_str().unwrap_or_default());
                let mut ids: Vec<&str> = ["reset", "url"]
                    .iter()
                    .filter_map(|k| m[*k].as_str())
                    .collect();
                ids.extend(
                    m["actions"]
                        .as_object()
                        .into_iter()
                        .flatten()
                        .filter_map(|(_, f)| f.as_str()),
                );
                for id in ids {
                    runner.calls.insert(
                        id.to_string(),
                        Used {
                            file: file.clone(),
                            name: id.rsplit("::").next().unwrap_or(id).to_string(),
                            call: found(object, id)?,
                        },
                    );
                }
                if let Some(id) = m["state"].as_str() {
                    runner.states.push(Read {
                        object: object.clone(),
                        call: found(object, id)?,
                    });
                }
                if let Some(id) = m["url"].as_str() {
                    let given = called(&found(object, id)?, Json::Null)?;
                    serves = Some(given.as_str().map_or(given.to_string(), String::from));
                }
            }
        }
        runner.hello = json!({ "adapter": adapter, "serves": serves });
        Ok(runner)
    }

    fn states(&self) -> Result<Json, String> {
        self.states
            .iter()
            .map(|r| Ok((r.object.clone(), called(&r.call, Json::Null)?)))
            .collect::<Result<serde_json::Map<_, _>, String>>()
            .map(Json::Object)
    }

    fn play(&mut self, turn: &Json) -> Json {
        let step = turn["step"].as_str();
        let mut ran = Ran::new();
        let mut result = Json::Null;
        for call in turn["calls"].as_array().into_iter().flatten() {
            let id = call["function"].as_str().unwrap_or_default();
            let Some(used) = self.calls.get(id) else {
                return json!({ "thrown": format!("the runner has no function `{id}`") });
            };
            ran.insert((used.file.clone(), used.name.clone()));
            match called(&used.call, call["input"].clone()) {
                Ok(value) => result = value,
                Err(message) => return json!({ "thrown": message }),
            }
        }
        match step {
            None => {}
            Some(START) => self.started.extend(ran),
            Some(verb) => self.ran.entry(verb.into()).or_default().extend(ran),
        }
        match self.states() {
            Ok(states) => json!({ "states": states, "result": result }),
            Err(e) => json!({ "thrown": e }),
        }
    }

    fn spans(ran: &Ran) -> Json {
        let cwd = fs::canonicalize(env::current_dir().unwrap_or_default()).unwrap_or_default();
        let mut out = Vec::new();
        for (file, function) in ran {
            let Ok(text) = fs::read_to_string(file) else {
                continue;
            };
            let Ok(parsed) = syn::parse_file(&text) else {
                continue;
            };
            for item in parsed.items {
                if let syn::Item::Fn(f) = item
                    && f.sig.ident == function
                {
                    let span = f.span();
                    out.push(json!({
                        "file": file.strip_prefix(&cwd).unwrap_or(file).display().to_string(),
                        "from": span.start().line,
                        "to": span.end().line,
                    }));
                }
            }
        }
        Json::Array(out)
    }

    fn ran(&self) -> Json {
        let actions: serde_json::Map<String, Json> = self
            .ran
            .iter()
            .map(|(verb, ran)| (verb.clone(), Self::spans(ran)))
            .collect();
        json!({ "start": Self::spans(&self.started), "actions": actions })
    }
}

fn joined(functions: &[Function]) -> Result<Runner, String> {
    let Given {
        diagram, adapter, ..
    } = Given::read()?;
    let code: Json = serde_json::from_str(&said(JOIN, diagram.as_bytes()).map_err(|(_, e)| e)?)
        .map_err(|e| e.to_string())?;
    Runner::new(&code, &adapter, functions)
}

fn played(runner: &mut Runner) -> Result<(), String> {
    let mut report = runner.hello.clone();
    loop {
        let turn: Json =
            serde_json::from_str(&said(TURN, report.to_string().as_bytes()).map_err(|(_, e)| e)?)
                .map_err(|e| e.to_string())?;
        if turn.get("done").is_some() {
            return Ok(());
        }
        report = runner.play(&turn);
    }
}

fn ran(functions: &[Function]) -> Result<(), String> {
    panic::set_hook(Box::new(|info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(ToString::to_string)
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let at = info
            .location()
            .map_or(String::new(), |l| format!(" at {}:{}", l.file(), l.line()));
        if let Ok(mut fault) = FAULT.lock() {
            *fault = Some(format!("panic: {message}{at}"));
        }
    }));
    let ended = joined(functions).and_then(|mut runner| {
        played(&mut runner)?;
        Ok(runner.ran())
    });
    let said_done = match &ended {
        Ok(ran) => json!({ "ran": ran }),
        Err(e) => json!({ "error": e }),
    };
    said(DONE, said_done.to_string().as_bytes())
        .map_err(|(_, e)| e)
        .and(ended.map(|_| ()))
}

pub fn run(functions: &[Function]) -> ExitCode {
    match ran(functions) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn tested(file: &str, functions: &[Function]) -> Result<(), String> {
    let weft = env::var(WEFT).unwrap_or_else(|_| "weft".into());
    let mut child = Command::new(&weft)
        .args([PARENT, ADAPTER, file])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{weft}: {e}"))?;
    let mut output = BufReader::new(child.stdout.take().ok_or("weft has no output")?);
    let mut first = String::new();
    output.read_line(&mut first).map_err(|e| e.to_string())?;
    let joined = serde_json::from_str::<Json>(&first).ok();
    let played = joined.as_ref().map_or(Ok(()), |given| {
        env::set_current_dir(given["cwd"].as_str().unwrap_or_default())
            .map_err(|e| e.to_string())?;
        for (key, value) in given["env"].as_object().into_iter().flatten() {
            unsafe { env::set_var(key, value.as_str().unwrap_or_default()) };
        }
        ran(functions)
    });
    let mut report = match joined {
        Some(_) => String::new(),
        None => first,
    };
    io::Read::read_to_string(&mut output, &mut report).map_err(|e| e.to_string())?;
    let status = child.wait().map_err(|e| e.to_string())?;
    match (status.success(), played) {
        (true, Ok(())) => {
            print!("{report}");
            Ok(())
        }
        (_, Err(e)) => Err(format!("{e}\n{report}")),
        (false, Ok(())) => Err(format!("{status}\n{report}")),
    }
}

pub fn test(file: &str, functions: &[Function]) {
    if let Err(e) = tested(file, functions) {
        panic!("{e}");
    }
}
