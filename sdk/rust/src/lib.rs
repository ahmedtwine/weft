use std::collections::{BTreeMap, BTreeSet, HashMap};
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

enum Export {
    Function(Call),
    Schema(Json),
}

pub struct Module {
    file: PathBuf,
    source: String,
    exports: HashMap<String, Export>,
}

#[macro_export]
macro_rules! module {
    () => {
        $crate::Module::at(env!("CARGO_MANIFEST_DIR"), file!())
    };
}

pub trait Function<M>: Send + Sync + 'static {
    fn call(&self, input: Json) -> Result<Json, Failure>;
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
}

impl<F, I, T, E> Function<(I,)> for F
where
    F: Fn(I) -> Result<T, E> + Send + Sync + 'static,
    I: DeserializeOwned,
    T: Serialize,
    E: Display,
{
    fn call(&self, input: Json) -> Result<Json, Failure> {
        let input = serde_json::from_value(input).map_err(|_| Failure::Refused)?;
        returned(self(input))
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
            exports: HashMap::new(),
        }
    }

    pub fn schema(mut self, name: &str, schema: Json) -> Module {
        self.exports.insert(name.into(), Export::Schema(schema));
        self
    }

    pub fn function<M>(mut self, name: &str, function: impl Function<M>) -> Module {
        let call: Call = Arc::new(move |input| function.call(input));
        self.exports.insert(name.into(), Export::Function(call));
        self
    }
}

struct Used {
    run: Option<Call>,
    function: (usize, String),
    input: bool,
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
    uses: Vec<Option<Used>>,
    hello: Json,
    start: Ran,
    actions: BTreeMap<String, Ran>,
}

static FAULT: Mutex<Option<String>> = Mutex::new(None);

fn place(at: &str) -> PathBuf {
    let path = env::current_dir().unwrap_or_default().join(at);
    fs::canonicalize(&path).unwrap_or(path)
}

fn name(node: &Json) -> Option<(String, String)> {
    Some((
        node.get(0)?.as_str()?.to_string(),
        node.get(1)?.as_str()?.to_string(),
    ))
}

impl Runner {
    fn new(code: &Json, adapter: &str, modules: Vec<Module>) -> Result<Runner, String> {
        let files: Vec<PathBuf> = modules.iter().map(|m| m.file.clone()).collect();
        let mut uses = Vec::new();
        let mut hello = Vec::new();
        let mut serves = None;
        let mut used: BTreeSet<usize> = BTreeSet::new();
        for i in code.as_array().into_iter().flatten() {
            let lines = i["uses"].as_array().map_or(&[][..], Vec::as_slice);
            if i["adapter"] != adapter {
                uses.extend(lines.iter().map(|_| None));
                hello.extend(lines.iter().map(|_| Json::Null));
                continue;
            }
            let mut found: HashMap<String, usize> = HashMap::new();
            for (module, at) in i["modules"].as_object().into_iter().flatten() {
                let at = at.as_str().unwrap_or_default();
                let index = files
                    .iter()
                    .position(|file| *file == place(at))
                    .ok_or_else(|| format!("the runner has no module `{at}`"))?;
                found.insert(module.clone(), index);
                used.insert(index);
            }
            let export = |(module, name): (String, String)| {
                let index = *found
                    .get(&module)
                    .ok_or_else(|| format!("`impl \"{adapter}\"` has no `module \"{module}\"`"))?;
                modules[index]
                    .exports
                    .get(&name)
                    .map(|e| (index, name.clone(), e))
                    .ok_or_else(|| format!("`{module}` exports no `{name}`"))
            };
            let schema = |node: &Json| match name(node) {
                None => Ok(None),
                Some(n) => match export(n.clone())? {
                    (_, _, Export::Schema(json)) => {
                        Ok(Some((format!("{}.{}", n.0, n.1), json.clone())))
                    }
                    _ => Err(format!("`{}.{}` is no schema", n.0, n.1)),
                },
            };
            if let Some(n) = name(&i["serve"]) {
                match export(n.clone())? {
                    (_, _, Export::Function(call)) => {
                        let given = Self::called(call, Json::Null).map_err(|f| match f {
                            Failure::Error(m) | Failure::Fault(m) => m,
                            Failure::Refused => "the `serve` function takes no input".into(),
                        })?;
                        serves = Some(given.as_str().map_or(given.to_string(), String::from));
                    }
                    _ => return Err(format!("`{}.{}` is no function", n.0, n.1)),
                }
            }
            for line in lines {
                if line["from"] == true {
                    return Err("the rust runner calls named functions, and a `from` or `consume` line needs a runner with objects".into());
                }
                let n = name(&line["function"]).ok_or("a line names no function")?;
                let (index, function, run) = match export(n.clone())? {
                    (index, function, Export::Function(call)) => (index, function, call.clone()),
                    _ => return Err(format!("`{}.{}` is no function", n.0, n.1)),
                };
                let input = schema(&line["input"])?;
                hello.push(json!({ "input": input.as_ref().map(|(_, j)| j) }));
                uses.push(Some(Used {
                    run: Some(run),
                    function: (index, function),
                    input: input.is_some(),
                }));
            }
        }
        let sources: Vec<&str> = used.iter().map(|i| modules[*i].source.as_str()).collect();
        let registered = STATES.lock().map_err(|e| e.to_string())?;
        let mine: Vec<&Registered> = registered
            .iter()
            .filter(|s| sources.contains(&s.source))
            .collect();
        Ok(Runner {
            hello: json!({
                "uses": hello,
                "serves": serves,
                "states": mine.iter().map(|s| (s.key.clone(), s.schema.clone())).collect::<serde_json::Map<_, _>>(),
            }),
            states: mine.iter().map(|s| s.key.clone()).collect(),
            files,
            uses,
            start: Ran::new(),
            actions: BTreeMap::new(),
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
        let mut added = Vec::new();
        for call in turn["calls"].as_array().into_iter().flatten() {
            let at = call["use"].as_u64().unwrap_or(u64::MAX) as usize;
            let used = self
                .uses
                .get(at)
                .and_then(Option::as_ref)
                .ok_or_else(|| format!("the diagram has no line {at} for this runner"))?;
            let input = match used.input {
                true => call["input"].clone(),
                false => Json::Null,
            };
            ran.insert(used.function.clone());
            let run = used.run.as_ref().ok_or("a line with no function")?;
            match Self::called(run, input) {
                Ok(_) => {
                    if let Some(add) = call["add"].as_u64() {
                        added.push(add);
                    }
                }
                Err(Failure::Refused) => return Ok(json!({ "refused": true })),
                Err(Failure::Error(message) | Failure::Fault(message)) => {
                    return Ok(json!({ "thrown": message }));
                }
            }
        }
        match step {
            None => {}
            Some(START) => self.start.extend(ran),
            Some(verb) => self.actions.entry(verb.into()).or_default().extend(ran),
        }
        Ok(match states(&self.states) {
            Ok(states) => json!({ "states": states, "added": added }),
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
            .actions
            .iter()
            .map(|(verb, ran)| (verb.clone(), self.spans(ran)))
            .collect();
        json!({ "start": self.spans(&self.start), "actions": actions })
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
