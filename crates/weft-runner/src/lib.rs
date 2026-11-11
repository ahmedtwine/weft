mod bun;
mod setup;
mod transport;

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde_json::{Value as Json, json};
use weft_parse::{Adapter, Config, Diagram, Name, Role};
use weft_simulator::{Member, Outcome, simulate};

use setup::Setup;
use transport::{HUB, listen};

const WAIT: Duration = Duration::from_secs(60);

fn verbs(d: &Diagram) -> Vec<String> {
    let mut all: Vec<String> = d
        .objects
        .iter()
        .flat_map(|o| &o.moves)
        .map(|m| m.verb.clone())
        .collect();
    all.sort_unstable();
    all.dedup();
    all
}

fn clean(path: &Path) -> String {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir if out.file_name().is_some() => {
                out.pop();
            }
            Component::CurDir => {}
            part => out.push(part),
        }
    }
    out.display().to_string()
}

fn code(d: &Diagram, file: &Path) -> String {
    let dir = file.parent().unwrap_or(Path::new(""));
    let role = |role: Role| match role {
        Role::Start => "start",
        Role::Read => "read",
        Role::Run => "run",
    };
    let place = |place: &str| match place.starts_with('.') {
        true => format!("./{}", clean(&dir.join(place))),
        false => place.to_string(),
    };
    let name = |n: &Name| json!([n.module, n.export]);
    Json::from_iter(d.impls.iter().map(|i| {
        json!({
            "adapter": i.adapter,
            "modules": Json::from_iter(i.modules.iter().map(|(module, at)| (module.clone(), place(at)))),
            "uses": Json::from_iter(i.uses.iter().map(|u| json!({
                "role": role(u.role),
                "function": name(&u.function),
                "input": u.input.as_ref().map(name),
                "output": u.output.as_ref().map(name),
                "error": u.error.as_ref().map(name),
                "from": u.from.is_some(),
            }))),
        })
    }))
    .to_string()
}

pub fn run(
    config: &Config,
    d: &Diagram,
    file: &Path,
    key: Option<&str>,
) -> Result<Outcome, String> {
    let file = file.strip_prefix(".").unwrap_or(file);
    if (&config.transport, &config.adapter) == (&None, &Adapter::Default) {
        return simulate(d, &mut [Box::new(Setup::new(d, file))], config.cases, key);
    }
    if d.impls.is_empty() {
        return Err("a runner needs `impl <adapter> { … }` in the diagram".into());
    }
    let name = file.display().to_string();
    let at = listen(config.transport.as_ref())?;
    let uses = d.impls.iter().flat_map(|i| i.uses.clone()).collect();
    let draws = d.objects.iter().flat_map(|o| o.draws.clone()).collect();
    let pools = d.objects.iter().flat_map(|o| o.pools.clone()).collect();
    let session = HUB.open(&name, code(d, file), uses, draws, pools);
    let bun = match config.adapter {
        Adapter::Bun => Some(bun::start(&name, &at, session.clone())?),
        Adapter::Default => None,
    };
    let outcome = session.members(&verbs(d), WAIT).and_then(|remotes| {
        let mut members: Vec<Box<dyn Member>> = remotes
            .into_iter()
            .map(|remote| Box::new(remote) as Box<dyn Member>)
            .collect();
        simulate(d, &mut members, config.cases, key)
    });
    HUB.close(&name);
    let log = bun.and_then(|bun| bun.join().ok());
    match (outcome, log) {
        (Err(e), Some(log)) => Err(format!("{e}: {log}")),
        (outcome, _) => outcome,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::env;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpStream;
    use std::os::unix::net::UnixStream;
    use std::sync::{Arc, Mutex};
    use std::thread;

    use weft_simulator::view;
    use weft_wire::{DONE, JOIN, Reader, TURN, Writer, receive, send};

    const FILE: &str = "invariants/page.weft.kdl";
    const CONFIG: &str = "include \"**/*.weft.kdl\"\ntheme \"light\"\nstyle \"person\" icon=\"user\"\nstyle \"browser\" icon=\"app-window\"\n";
    const PAGE: &str = r#"
setup {
  person "user"
  browser "page" count=0 {DRAW
    state count=0 {
      on "send" count=1
    }
    state count=1 {
      on "reload" count=1
    }
  }
}
action {
  send from="user" to="page"
  reload from="user" to="page" page.count=0
}
check {
  invariant {
    forall "browser" "page" {
      implies {
        in "step" "reload"
        eq (next)"browser.count" "browser.count"
      }
    }
  }
}
impl "bun" {
  module "page" "../src/page.ts"
  start "page.clear" input="page.clearSchema"
  read "page.page" output="page.pageSchema"
  run "page.send" input="page.sendSchema"
  run "page.reload" input="page.reloadSchema"
}
"#;
    const NONE: &str = r#"{"type":"object","properties":{},"additionalProperties":false}"#;
    const NUMBER: &str = r#"{"type":"object","properties":{"n":{"type":"integer","minimum":0,"maximum":1000}},"required":["n"],"additionalProperties":false}"#;
    const COUNT: &str = r#"{"type":"object","properties":{"count":{"type":"integer","weft":{"read":"page.count"}}}}"#;

    const POOL: &str = r#"
setup {
  person "user"
  browser "page" count=0 opened=0 {
    pool "opened"
    state count=0 opened=0 {
      on "open" count=1 opened=1
    }
    state count=1 opened=1 {
      on "open" count=1 opened=1
      on "close" count=0 opened=0
    }
  }
}
action {
  open from="user" to="page"
  close from="user" to="page" page.count=1
}
check {
  invariant {
    forall "browser" "page" {
      implies {
        eq (next)"browser.opened" 0
        eq (next)"browser.count" (start)"browser.count"
      }
    }
  }
}
impl "bun" {
  module "page" "../src/page.ts"
  start "page.clear" input="page.clearSchema"
  read "page.page" output="page.pageSchema"
  run "page.open" input="page.openSchema" pool="opened"
  run "opened" consume="opened" verb="close"
}
"#;

    #[derive(Default)]
    struct Page {
        count: i64,
        added: HashMap<u64, i64>,
    }

    type App = Arc<Mutex<Page>>;
    type Play = fn(&App, &Json) -> Json;

    enum Link {
        Unix(UnixStream),
        Http(String),
    }

    fn post(url: &str, body: &[u8]) -> Result<Vec<u8>, String> {
        let host = url.trim_start_matches("http://");
        let (address, path) = host.split_once('/').unwrap_or((host, ""));
        let mut stream = TcpStream::connect(address).map_err(|e| e.to_string())?;
        write!(
            stream,
            "POST /{path} HTTP/1.1\r\nhost: {address}\r\ncontent-length: {}\r\n\r\n",
            body.len()
        )
        .and_then(|()| stream.write_all(body))
        .map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(stream);
        let mut size = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).map_err(|e| e.to_string())?;
            match line.trim().split_once(':') {
                Some((name, value)) if name.eq_ignore_ascii_case("content-length") => {
                    size = value.trim().parse().map_err(|_| "size")?;
                }
                Some(_) => {}
                None if line.trim().is_empty() => break,
                None => {}
            }
        }
        let mut body = vec![0; size];
        reader.read_exact(&mut body).map_err(|e| e.to_string())?;
        Ok(body)
    }

    impl Link {
        fn said(&mut self, call: u8, part: &str) -> Result<String, String> {
            let message = Writer::call(call, part.as_bytes()).0;
            let reply = match self {
                Link::Unix(stream) => send(stream, &message)
                    .and_then(|()| receive(stream))
                    .map_err(|e| e.to_string())?,
                Link::Http(url) => post(url, &message)?,
            };
            let mut r = Reader(&reply);
            let code = r.code().map_err(|_| "short")?;
            let text = String::from_utf8_lossy(r.bytes().map_err(|_| "short")?).into_owned();
            match code {
                0 => Ok(text),
                _ => Err(text),
            }
        }
    }

    fn state(app: &App) -> Json {
        let (count, added) = app.lock().map_or((0, Vec::new()), |page| {
            (page.count, page.added.keys().copied().collect())
        });
        json!({ "outputs": { "1": { "count": count } }, "errors": {}, "added": added })
    }

    fn runner(at: &str, hello: Json, app: App, play: Play, verbs: [&str; 2]) -> Result<(), String> {
        let mut link = match at.strip_prefix("unix:") {
            Some(path) => Link::Unix(UnixStream::connect(path).map_err(|e| e.to_string())?),
            None => Link::Http(format!("{at}/{}", hello.to_string().len())),
        };
        let joined = (0..500)
            .find_map(|_| {
                link.said(JOIN, FILE)
                    .inspect_err(|_| thread::sleep(Duration::from_millis(10)))
                    .ok()
            })
            .ok_or("weft opens no diagram")?;
        let code: Json = serde_json::from_str(&joined).map_err(|e| e.to_string())?;
        assert_eq!(code[0]["adapter"], "bun");
        assert_eq!(code[0]["modules"]["page"], "./src/page.ts");
        assert_eq!(code[0]["uses"][0]["function"], json!(["page", "clear"]));
        let mut report = hello;
        loop {
            let turn: Json = serde_json::from_str(&link.said(TURN, &report.to_string())?)
                .map_err(|e| e.to_string())?;
            if turn.get("done").is_some() {
                break;
            }
            report = play(&app, &turn);
        }
        let span = json!([{ "file": "src/page.ts", "from": 1, "to": 2 }]);
        link.said(
            DONE,
            &json!({ "ran": { "start": span, "actions": { verbs[0]: span, verbs[1]: span } } })
                .to_string(),
        )?;
        Ok(())
    }

    fn set(app: &App, to: impl FnOnce(i64) -> i64) {
        if let Ok(mut page) = app.lock() {
            page.count = to(page.count);
        }
    }

    fn opener(app: &App, turn: &Json, closes: fn(i64) -> bool) -> Json {
        let call = &turn["calls"][0];
        if let Ok(mut page) = app.lock() {
            match (
                turn["step"].as_str(),
                call["add"].as_u64(),
                call["from"].as_u64(),
            ) {
                (Some("start"), ..) => *page = Page::default(),
                (Some("open"), Some(add), _) => {
                    let n = call["input"]["n"].as_i64().unwrap_or(0);
                    page.count += n;
                    page.added.insert(add, n);
                }
                (Some("close"), _, Some(from)) => {
                    let n = page.added.remove(&from).unwrap_or(i64::MAX);
                    page.count -= if closes(n) { n } else { 0 };
                }
                _ => {}
            }
        }
        state(app)
    }

    fn pooled(name: &str, play: Play) -> Result<Outcome, String> {
        let config = weft_parse::config(&format!(
            "{CONFIG}{}\nadapter \"default\" cases=100\n",
            socket(name)
        ))
        .map_err(|e| e.to_string())?;
        let d = weft_parse::parse(POOL, &config).map_err(|e| e.to_string())?;
        let at = listen(config.transport.as_ref())?;
        let hello = json!({ "uses": [input(None, NONE), { "output": schema(COUNT) }, input(Some("open"), NUMBER), {}] });
        let opener =
            thread::spawn(move || runner(&at, hello, App::default(), play, ["open", "close"]));
        let outcome = run(&config, &d, Path::new(FILE), None);
        opener.join().map_err(|_| "a runner panics")??;
        outcome
    }

    fn sender(app: &App, turn: &Json) -> Json {
        match (
            turn["step"].as_str(),
            turn["calls"][0]["input"]["n"].as_i64(),
        ) {
            (Some("start"), _) => set(app, |_| 0),
            (_, Some(n)) => set(app, |_| n),
            _ => {}
        }
        state(app)
    }

    fn reloader(app: &App, turn: &Json) -> Json {
        if turn["step"] == "reload" {
            set(app, |n| if n > 5 { 0 } else { n });
        }
        json!({ "outputs": {}, "errors": {} })
    }

    fn reloader_with_no_change(_: &App, _: &Json) -> Json {
        json!({ "outputs": {}, "errors": {} })
    }

    fn schema(text: &str) -> Json {
        serde_json::from_str(text).unwrap_or_default()
    }

    fn input(verb: Option<&str>, text: &str) -> Json {
        let mut schema = schema(text);
        if let Some(verb) = verb {
            schema["weft"] = json!({ "verb": verb });
        }
        json!({ "input": schema })
    }

    fn sends() -> Json {
        json!({ "uses": [input(None, NONE), { "output": schema(COUNT) }, input(Some("send"), NUMBER), null] })
    }

    fn reloads() -> Json {
        json!({ "uses": [null, null, null, input(Some("reload"), NONE)] })
    }

    fn two(transport: &str, draw: &str, sends: Json, reload: Play) -> Result<Outcome, String> {
        let config = weft_parse::config(&format!(
            "{CONFIG}{transport}\nadapter \"default\" cases=100\n"
        ))
        .map_err(|e| e.to_string())?;
        let d =
            weft_parse::parse(&PAGE.replace("DRAW", draw), &config).map_err(|e| e.to_string())?;
        let at = listen(config.transport.as_ref())?;
        let app = App::default();
        let runners = [(sends, sender as Play), (reloads(), reload)].map(|(hello, play)| {
            let (at, app) = (at.clone(), app.clone());
            thread::spawn(move || runner(&at, hello, app, play, ["send", "reload"]))
        });
        let outcome = run(&config, &d, Path::new(FILE), None);
        for r in runners {
            let _ = r.join().map_err(|_| "a runner panics")?;
        }
        outcome
    }

    fn socket(name: &str) -> String {
        format!(
            "transport \"unix\" path=\"{}\"",
            env::temp_dir()
                .join(format!("weft-{name}-{}.sock", std::process::id()))
                .display()
        )
    }

    fn steps(o: &Outcome) -> Vec<(String, String)> {
        o.trace
            .iter()
            .map(|l| (l.step(), l.input.clone()))
            .collect()
    }

    #[test]
    fn two_runners_on_a_unix_socket_get_each_input_from_weft() -> Result<(), String> {
        let o = two(&socket("pass"), "", sends(), reloader_with_no_change)?;
        assert!(!o.failed);
        assert!(o.tally.iter().all(|t| t.steps > 0), "each verb runs");
        Ok(())
    }

    #[test]
    fn a_failing_case_of_two_runners_shrinks_to_the_least_value_and_steps() -> Result<(), String> {
        let o = two(&socket("shrink"), "", sends(), reloader)?;
        assert!(o.failed);
        assert_eq!(
            steps(&o),
            [
                ("start".to_string(), String::new()),
                ("send to=page".to_string(), "n=6".to_string()),
                ("reload to=page".to_string(), String::new()),
            ]
        );
        Ok(())
    }

    #[test]
    fn a_draw_line_gives_the_values_of_a_field() -> Result<(), String> {
        let o = two(
            &socket("low"),
            "\n    draw {\n      n 1 2\n    }",
            sends(),
            reloader,
        )?;
        assert!(!o.failed);
        let o = two(
            &socket("high"),
            "\n    draw {\n      n 7 9\n    }",
            sends(),
            reloader,
        )?;
        assert_eq!(steps(&o)[1].1, "n=7");
        Ok(())
    }

    #[test]
    fn an_added_result_of_a_pool_runs_in_a_later_step() -> Result<(), String> {
        let o = pooled("pool", |app, turn| opener(app, turn, |_| true))?;
        assert!(!o.failed);
        assert!(o.tally.iter().all(|t| t.steps > 0), "each verb runs");
        assert!(
            o.trace
                .iter()
                .any(|l| view(&l.state).contains("opened = 2")),
            "the state has the count of the pool"
        );
        Ok(())
    }

    #[test]
    fn a_failing_case_with_a_pool_shrinks_to_one_added_result() -> Result<(), String> {
        let o = pooled("leak", |app, turn| opener(app, turn, |n| n <= 5))?;
        assert!(o.failed);
        assert_eq!(
            steps(&o),
            [
                ("start".to_string(), String::new()),
                ("open to=page".to_string(), "n=6".to_string()),
                ("close to=page".to_string(), "0".to_string()),
            ]
        );
        assert_eq!(
            o.trace[2].evidence,
            [
                "browser = page",
                "eq(0, 0) = true",
                "eq(next(browser.count), start(browser.count)) = eq(6, 0) = false"
            ]
        );
        Ok(())
    }

    #[test]
    fn two_runners_over_http_get_each_input_from_weft() -> Result<(), String> {
        let o = two(
            "transport \"http\" url=\"http://127.0.0.1:7413\"",
            "",
            sends(),
            reloader_with_no_change,
        )?;
        assert!(!o.failed);
        Ok(())
    }

    #[test]
    fn the_setup_adapter_runs_the_diagram_without_the_code() -> Result<(), String> {
        let config = weft_parse::config(&format!("{CONFIG}adapter \"default\" cases=50\n"))
            .map_err(|e| e.to_string())?;
        let d = weft_parse::parse(&PAGE.replace("DRAW", ""), &config).map_err(|e| e.to_string())?;
        let o = run(&config, &d, Path::new(FILE), None)?;
        assert!(!o.failed);
        assert_eq!(o.cases, 50);
        Ok(())
    }

    #[test]
    fn weft_refuses_a_module_in_the_directory_of_the_diagram() -> Result<(), String> {
        let config = weft_parse::config(&format!("{CONFIG}adapter \"default\" cases=50\n"))
            .map_err(|e| e.to_string())?;
        let page = PAGE
            .replace("DRAW", "")
            .replace("../src/page.ts", "./page.ts");
        let error = weft_parse::parse(&page, &config)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("`module page` is a file in the directory of the diagram"),
            "{error}"
        );
        Ok(())
    }

    #[test]
    fn weft_refuses_a_run_schema_with_no_verb() {
        let hello = json!({ "uses": [input(None, NONE), { "output": schema(COUNT) }, input(None, NUMBER), null] });
        let error = two(&socket("refuse"), "", hello, reloader)
            .err()
            .unwrap_or_default();
        assert!(
            error.contains(
                "`run page.send input=page.sendSchema` is a `run`, and its schema has no `verb` annotation"
            ),
            "{error}"
        );
    }

    #[test]
    fn weft_refuses_a_draw_line_that_names_no_field() {
        let error = two(
            &socket("field"),
            "\n    draw {\n      m 1\n    }",
            sends(),
            reloader,
        )
        .err()
        .unwrap_or_default();
        assert!(
            error.contains("the line `m` of `draw` names a field of no input"),
            "{error}"
        );
    }
}
