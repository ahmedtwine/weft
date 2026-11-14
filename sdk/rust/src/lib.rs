use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt::Display;
use std::fs;
use std::panic::{self, AssertUnwindSafe, Location};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

pub use schemars::JsonSchema;
use serde::Serialize;
use serde::de::DeserializeOwned;
pub use serde_json::{Value as Json, json};
use syn::spanned::Spanned;
use weft_wire::client::said;
use weft_wire::{DONE, JOIN, TURN};

const ADAPTER: &str = "rust";
const START: &str = "start";

pub enum Failure {
    Refused,
    Error(String),
    Fault(String),
}

type Call = Arc<dyn Fn(Json) -> Result<Json, Failure> + Send + Sync>;

const RESET: &str = "reset";
const URL: &str = "url";

struct Export {
    name: String,
    call: Call,
    input: Option<Json>,
}

pub struct Module {
    file: PathBuf,
    source: String,
    exports: Vec<Export>,
}

#[macro_export]
macro_rules! module {
    () => {
        $crate::Module::at(env!("CARGO_MANIFEST_DIR"), file!())
    };
}

pub trait Function<M>: Send + Sync + 'static {
    fn call(&self, input: Json) -> Result<Json, Failure>;
    fn input(&self) -> Option<Json>;
}

fn returned<T: Serialize, E: Display>(result: Result<T, E>) -> Result<Json, Failure> {
    match result {
        Ok(value) => serde_json::to_value(value).map_err(|e| Failure::Fault(e.to_string())),
        Err(e) => Err(Failure::Error(e.to_string())),
    }
}

impl<F, T, E> Function<()> for F
where
    F: Fn() -> Result<T, E> + Send + Sync + 'static,
    T: Serialize,
    E: Display,
{
    fn call(&self, _: Json) -> Result<Json, Failure> {
        returned(self())
    }

    fn input(&self) -> Option<Json> {
        None
    }
}

impl<F, I, T, E> Function<(I,)> for F
where
    F: Fn(I) -> Result<T, E> + Send + Sync + 'static,
    I: DeserializeOwned + JsonSchema,
    T: Serialize,
    E: Display,
{
    fn call(&self, input: Json) -> Result<Json, Failure> {
        let input = serde_json::from_value(input).map_err(|_| Failure::Refused)?;
        returned(self(input))
    }

    fn input(&self) -> Option<Json> {
        serde_json::to_value(schemars::schema_for!(I)).ok()
    }
}

impl Module {
    pub fn at(manifest: &str, file: &str) -> Module {
        let found = Path::new(manifest)
            .ancestors()
            .map(|dir| dir.join(file))
            .find(|path| path.is_file())
            .unwrap_or_else(|| PathBuf::from(file));
        Module {
            file: fs::canonicalize(&found).unwrap_or(found),
            source: file.to_string(),
            exports: Vec::new(),
        }
    }

    pub fn function<M>(mut self, name: &str, function: impl Function<M>) -> Module {
        let input = function.input();
        let call: Call = Arc::new(move |input| function.call(input));
        self.exports.retain(|e| e.name != name);
        self.exports.push(Export {
            name: name.into(),
            call,
            input,
        });
        self
    }
}

struct Used {
    module: usize,
    name: String,
    call: Call,
}

type Read = Box<dyn Fn() -> Result<Json, String> + Send>;

struct Registered {
    key: String,
    source: &'static str,
    schema: Json,
    read: Read,
}

static STATES: Mutex<Vec<Registered>> = Mutex::new(Vec::new());

#[track_caller]
pub fn state<T: Serialize + JsonSchema>(key: &str, read: impl Fn() -> T + Send + 'static) {
    let registered = Registered {
        key: key.to_string(),
        source: Location::caller().file(),
        schema: serde_json::to_value(schemars::schema_for!(T)).unwrap_or(Json::Null),
        read: Box::new(move || serde_json::to_value(read()).map_err(|e| e.to_string())),
    };
    if let Ok(mut all) = STATES.lock() {
        all.retain(|known| known.key != key);
        all.push(registered);
    }
}

fn states(keys: &[String]) -> Result<Json, String> {
    let all = STATES.lock().map_err(|e| e.to_string())?;
    all.iter()
        .filter(|s| keys.contains(&s.key))
        .map(|s| Ok((s.key.clone(), (s.read)()?)))
        .collect::<Result<serde_json::Map<_, _>, String>>()
        .map(Json::Object)
}

type Ran = BTreeSet<(usize, String)>;

struct Runner {
    files: Vec<PathBuf>,
    states: Vec<String>,
    start: Vec<Used>,
    actions: Vec<Used>,
    hello: Json,
    started: Ran,
    ran: BTreeMap<String, Ran>,
}

static FAULT: Mutex<Option<String>> = Mutex::new(None);

fn place(at: &str) -> PathBuf {
    let path = env::current_dir().unwrap_or_default().join(at);
    fs::canonicalize(&path).unwrap_or(path)
}

impl Runner {
    fn new(code: &Json, adapter: &str, modules: Vec<Module>) -> Result<Runner, String> {
        let files: Vec<PathBuf> = modules.iter().map(|m| m.file.clone()).collect();
        let (mut start, mut actions) = (Vec::new(), Vec::new());
        let (mut said_start, mut said_actions) = (Vec::new(), Vec::new());
        let mut serves = None;
        let mut sources = Vec::new();
        let mine = code
            .as_array()
            .into_iter()
            .flatten()
            .filter(|i| i["adapter"] == adapter);
        for i in mine {
            for (module, at) in i["modules"].as_object().into_iter().flatten() {
                let at = at.as_str().unwrap_or_default();
                let index = files
                    .iter()
                    .position(|file| *file == place(at))
                    .ok_or_else(|| format!("the runner has no module `{at}`"))?;
                sources.push(modules[index].source.clone());
                for e in &modules[index].exports {
                    let used = Used {
                        module: index,
                        name: e.name.clone(),
                        call: e.call.clone(),
                    };
                    let said = json!({ "function": [module, e.name], "input": e.input });
                    match e.name.as_str() {
                        URL => {
                            let given = Self::called(&e.call, Json::Null).map_err(|f| match f {
                                Failure::Error(m) | Failure::Fault(m) => m,
                                Failure::Refused => "`url` takes no input".into(),
                            })?;
                            serves = Some(given.as_str().map_or(given.to_string(), String::from));
                        }
                        RESET => {
                            start.push(used);
                            said_start.push(said);
                        }
                        _ => {
                            actions.push(used);
                            said_actions.push(said);
                        }
                    }
                }
            }
        }
        let registered = STATES.lock().map_err(|e| e.to_string())?;
        let states: Vec<&Registered> = registered
            .iter()
            .filter(|s| sources.iter().any(|source| source == s.source))
            .collect();
        Ok(Runner {
            hello: json!({
                "adapter": adapter,
                "serves": serves,
                "start": said_start,
                "actions": said_actions,
                "states": states.iter().map(|s| (s.key.clone(), s.schema.clone())).collect::<serde_json::Map<_, _>>(),
            }),
            states: states.iter().map(|s| s.key.clone()).collect(),
            files,
            start,
            actions,
            started: Ran::new(),
            ran: BTreeMap::new(),
        })
    }

    fn called(run: &Call, input: Json) -> Result<Json, Failure> {
        match panic::catch_unwind(AssertUnwindSafe(|| run(input))) {
            Ok(result) => result,
            Err(payload) => Err(Failure::Fault(
                FAULT
                    .lock()
                    .ok()
                    .and_then(|mut f| f.take())
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "a panic".into()),
            )),
        }
    }

    fn play(&mut self, turn: &Json) -> Result<Json, String> {
        let step = turn["step"].as_str();
        let mut ran = Ran::new();
        for call in turn["calls"].as_array().into_iter().flatten() {
            let at = call["use"].as_u64().unwrap_or(u64::MAX) as usize;
            let list = match step {
                Some(START) => &self.start,
                _ => &self.actions,
            };
            let used = list
                .get(at)
                .ok_or_else(|| format!("the runner has no function {at}"))?;
            ran.insert((used.module, used.name.clone()));
            match Self::called(&used.call, call["input"].clone()) {
                Ok(_) => {}
                Err(Failure::Refused) => return Ok(json!({ "refused": true })),
                Err(Failure::Error(message) | Failure::Fault(message)) => {
                    return Ok(json!({ "thrown": message }));
                }
            }
        }
        match step {
            None => {}
            Some(START) => self.started.extend(ran),
            Some(verb) => self.ran.entry(verb.into()).or_default().extend(ran),
        }
        Ok(match states(&self.states) {
            Ok(states) => json!({ "states": states }),
            Err(e) => json!({ "thrown": e }),
        })
    }

    fn spans(&self, ran: &Ran) -> Json {
        let cwd = fs::canonicalize(env::current_dir().unwrap_or_default()).unwrap_or_default();
        let mut out = Vec::new();
        for (index, function) in ran {
            let file = &self.files[*index];
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
            .map(|(verb, ran)| (verb.clone(), self.spans(ran)))
            .collect();
        json!({ "start": self.spans(&self.started), "actions": actions })
    }
}

fn joined(modules: Vec<Module>) -> Result<Runner, String> {
    let adapter = env::var("WEFT_ADAPTER").unwrap_or_else(|_| ADAPTER.into());
    let diagram = env::var("WEFT_DIAGRAM").map_err(|_| "WEFT_DIAGRAM is not set".to_string())?;
    let code: Json =
        serde_json::from_str(&said(JOIN, diagram.as_bytes())?).map_err(|e| e.to_string())?;
    Runner::new(&code, &adapter, modules)
}

fn played(runner: &mut Runner) -> Result<(), String> {
    let mut report = runner.hello.clone();
    loop {
        let turn: Json = serde_json::from_str(&said(TURN, report.to_string().as_bytes())?)
            .map_err(|e| e.to_string())?;
        if turn.get("done").is_some() {
            return Ok(());
        }
        report = runner.play(&turn)?;
    }
}

pub fn run(modules: impl IntoIterator<Item = Module>) -> ExitCode {
    panic::set_hook(Box::new(|info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let at = info
            .location()
            .map_or(String::new(), |l| format!(" at {}:{}", l.file(), l.line()));
        if let Ok(mut fault) = FAULT.lock() {
            *fault = Some(format!("panic: {message}{at}"));
        }
    }));
    let ended = joined(modules.into_iter().collect()).and_then(|mut runner| {
        played(&mut runner)?;
        Ok(runner.ran())
    });
    let said_done = match &ended {
        Ok(ran) => json!({ "ran": ran }),
        Err(e) => json!({ "error": e }),
    };
    match (said(DONE, said_done.to_string().as_bytes()), ended) {
        (Ok(_), Ok(_)) => ExitCode::SUCCESS,
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
