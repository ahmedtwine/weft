mod hub;
mod invariant;
mod process;
mod protocol;
mod proxy;
mod reaper;
mod report;
mod schema;
mod shape;
mod stateful;
mod transport;

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use std::{env, fs};

use serde_json::{Value as Json, json};
use weft_parse::{Adapter, Config, Diagram, Start};

pub use proxy::Hop;
pub use reaper::{REAP, guard, reap};
pub use report::{brief, checks, compact, sorted, summary};
pub use stateful::{Event, Line, Outcome, Stream, Tally, view};
use transport::{HUB, listen};

const WAIT: Duration = Duration::from_secs(60);
pub const CONFIG: &str = "weft.kdl";

pub fn config() -> Result<Config, String> {
    let source =
        fs::read_to_string(CONFIG).map_err(|_| format!("{CONFIG} is not in this directory"))?;
    weft_parse::config(&source).map_err(|e| format!("{CONFIG}: {e}"))
}

pub fn rooted() -> Result<(), String> {
    let cwd = env::current_dir().map_err(|e| e.to_string())?;
    let root = cwd
        .ancestors()
        .find(|dir| dir.join(CONFIG).is_file())
        .ok_or_else(|| format!("{CONFIG} is not in this directory or in a directory above it"))?;
    env::set_current_dir(root).map_err(|e| e.to_string())
}

pub fn diagram(file: &Path, config: &Config) -> Result<Diagram, String> {
    let src = fs::read_to_string(file).map_err(|e| e.to_string())?;
    weft_parse::parse(&src, config).map_err(|e| e.to_string())
}

pub fn tested<'a>(
    config: &Config,
    d: &'a Diagram,
    file: &Path,
    parent: Option<&str>,
    stream: &'a mut Stream<'a>,
) -> Result<Outcome, String> {
    let key = fs::canonicalize(file).map_err(|e| e.to_string())?;
    run(config, d, file, key.to_str(), parent, stream)
}

pub fn totals(lines: &[String], checks: u64, took: Duration) -> String {
    let fail = lines.iter().filter(|l| l.starts_with('✕')).count();
    format!(
        "{}\n\n {} pass\n {fail} fail\n {checks} checks\nRan {} in {:.2}s",
        lines.join("\n"),
        lines.len() - fail,
        match lines.len() {
            1 => "1 file".to_string(),
            n => format!("{n} files"),
        },
        took.as_secs_f64()
    )
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
    let place = |place: &str| match place.starts_with('.') {
        true => format!("./{}", clean(&dir.join(place))),
        false => place.to_string(),
    };
    Json::from_iter(d.impls.iter().map(|i| {
        json!({
            "adapter": i.adapter,
            "modules": Json::from_iter(i.modules.iter().map(|(module, at)| (module.clone(), place(at)))),
        })
    }))
    .to_string()
}

pub fn run<'a>(
    config: &Config,
    d: &'a Diagram,
    file: &Path,
    key: Option<&str>,
    parent: Option<&str>,
    stream: &'a mut Stream<'a>,
) -> Result<Outcome, String> {
    let file = file.strip_prefix(".").unwrap_or(file);
    if d.impls.is_empty() {
        return Err("a runner needs `impl \"<adapter>\" { … }` in the diagram".into());
    }
    let name = file.display().to_string();
    let at = listen(config.transport.as_ref())?;
    let draws = d.objects.iter().flat_map(|o| o.draws.clone()).collect();
    let impls = d.impls.iter().map(|i| i.adapter.clone()).collect();
    let objects = d.objects.iter().map(|o| o.name.clone()).collect();
    let session = HUB.open(&name, code(d, file), impls, objects, draws);
    let routes: proxy::Routes = Arc::new(
        d.impls
            .iter()
            .flat_map(|i| {
                i.modules
                    .iter()
                    .map(|(m, _)| (m.clone(), i.adapter.clone()))
            })
            .collect(),
    );
    let mut started = Vec::new();
    let mut proxies = Vec::new();
    for i in &d.impls {
        let joins = parent == Some(i.adapter.as_str());
        let line = config
            .runners
            .iter()
            .find(|(adapter, _)| *adapter == i.adapter && !joins)
            .map(|(_, start)| start.clone())
            .or_else(|| {
                (i.adapter == "bun" && config.adapter == Adapter::Bun && !joins)
                    .then_some(Start::Bun)
            });
        if line.is_none() && !joins {
            continue;
        }
        let app = i
            .modules
            .first()
            .map_or(i.adapter.as_str(), |(m, _)| m.as_str());
        let proxy = match proxy::open(app, session.clone(), routes.clone()) {
            Ok(proxy) => proxy,
            Err(e) => {
                HUB.close(&name);
                return Err(e);
            }
        };
        let env: Vec<(String, String)> = routes
            .iter()
            .map(|(app, _)| {
                let key: String = app
                    .chars()
                    .map(|c| match c.is_ascii_alphanumeric() {
                        true => c.to_ascii_uppercase(),
                        false => '_',
                    })
                    .collect();
                (format!("WEFT_{key}_URL"), format!("{}/{app}", proxy.url))
            })
            .collect();
        let r = process::Runner {
            adapter: &i.adapter,
            name: &name,
            at: &at,
            env: &env,
            session: session.clone(),
        };
        proxies.push(proxy);
        let runner = match line {
            Some(Start::Command(line)) => process::command(&line, r).map(Some),
            Some(Start::Bun) => process::bun(r).map(Some),
            None => process::parent(&r).map(|line| {
                stream(Event::Joined(&line));
                None
            }),
        };
        match runner {
            Ok(runner) => started.extend(runner),
            Err(e) => {
                HUB.close(&name);
                process::ended(started);
                return Err(e);
            }
        }
    }
    let joined = session.members(WAIT);
    let outcome = match joined {
        Ok(mut members) => Ok(d
            .actions(file.parent().unwrap_or(Path::new("")))
            .and_then(|declared| stateful::run(d, &name, &mut members, key, &declared, stream))),
        Err(e) => Err(e),
    };
    HUB.close(&name);
    drop(proxies);
    let logs = process::ended(started);
    match outcome {
        Ok(outcome) => outcome,
        Err(e) if !logs.is_empty() => Err(format!("{e}: {}", logs.join("; "))),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpStream;
    use std::os::unix::net::UnixStream;
    use std::sync::{Arc, Mutex};
    use std::thread;

    use weft_wire::{
        ASSUME, DONE, JOIN, OK, POOL_ADD, POOL_GENERATE, REFUSED, Reader, STOP_TEST, TARGET, TURN,
        Writer, receive, send,
    };

    static FILES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn file() -> String {
        format!(
            "invariants/page-{}.weft.kdl",
            FILES.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )
    }
    const CONFIG: &str = "include \"**/*.weft.kdl\"\nstyle \"person\" icon=\"user\"\nstyle \"browser\" icon=\"app-window\"\n";
    const PAGE: &str = r#"
setup {
  person "user"
  browser "page" count=0 {DRAW
    invariant "counted" {
      igte "page.count" 0
    }
  }
  browser "tab"
}
impl "bun" {
  module "page" "../fixtures/page.ts"
}
impl "other" {
  module "tab" "../fixtures/tab.ts"
}
"#;
    const NONE: &str = r#"{"type":"object","properties":{},"additionalProperties":false}"#;
    const NUMBER: &str = r#"{"type":"object","properties":{"n":{"type":"integer","minimum":0,"maximum":1000}},"required":["n"],"additionalProperties":false}"#;

    #[derive(Default)]
    struct Page {
        count: i64,
    }

    type App = Arc<Mutex<Page>>;
    type Play = fn(&App, &Json, &mut Link) -> Json;

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
                None if line.trim().is_empty() => break,
                _ => {}
            }
        }
        let mut body = vec![0; size];
        reader.read_exact(&mut body).map_err(|e| e.to_string())?;
        Ok(body)
    }

    impl Link {
        fn asked(&mut self, call: u8, part: &str) -> Result<String, (i32, String)> {
            let refused = |e: String| (REFUSED, e);
            let message = Writer::call(call, part.as_bytes()).0;
            let reply = match self {
                Link::Unix(stream) => send(stream, &message)
                    .and_then(|()| receive(stream))
                    .map_err(|e| refused(e.to_string()))?,
                Link::Http(url) => post(url, &message).map_err(refused)?,
            };
            let mut r = Reader(&reply);
            let code = r.code().map_err(|_| refused("short".into()))?;
            let text = String::from_utf8_lossy(r.bytes().map_err(|_| refused("short".into()))?)
                .into_owned();
            match code {
                OK => Ok(text),
                code => Err((code, text)),
            }
        }

        fn said(&mut self, call: u8, part: &str) -> Result<String, String> {
            self.asked(call, part).map_err(|(_, e)| e)
        }
    }

    fn state(app: &App) -> Json {
        let count = app.lock().map_or(0, |page| page.count);
        json!({ "states": { "page.count": count } })
    }

    fn runner(
        at: &str,
        file: &str,
        hello: &Json,
        app: &App,
        play: Play,
        verbs: [&str; 2],
    ) -> Result<(), String> {
        let mut link = match at.strip_prefix("unix:") {
            Some(path) => Link::Unix(UnixStream::connect(path).map_err(|e| e.to_string())?),
            None => Link::Http(format!("{at}/{}", hello.to_string().len())),
        };
        let joined = (0..500)
            .find_map(|_| {
                link.said(JOIN, file)
                    .inspect_err(|_| thread::sleep(Duration::from_millis(10)))
                    .ok()
            })
            .ok_or("weft opens no diagram")?;
        let code: Json = serde_json::from_str(&joined).map_err(|e| e.to_string())?;
        assert_eq!(code[0]["adapter"], "bun");
        assert_eq!(code[0]["modules"]["page"], "./fixtures/page.ts");
        let mut report = hello.to_string();
        loop {
            let turn: Json =
                serde_json::from_str(&link.said(TURN, &report)?).map_err(|e| e.to_string())?;
            if turn.get("done").is_some() {
                break;
            }
            report = play(app, &turn, &mut link).to_string();
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

    fn sender(app: &App, turn: &Json, _: &mut Link) -> Json {
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

    fn reloader(app: &App, turn: &Json, _: &mut Link) -> Json {
        if turn["step"] == "reload" {
            set(app, |n| if n > 5 { -1 } else { n });
        }
        json!({ "states": {} })
    }

    fn reloader_with_no_change(_: &App, _: &Json, _: &mut Link) -> Json {
        json!({ "states": {} })
    }

    fn ended(asked: Result<String, (i32, String)>, state: Json) -> Json {
        match asked {
            Ok(_) => state,
            Err((ASSUME, _)) => json!({ "refused": "assume" }),
            Err((STOP_TEST, _)) => json!({ "refused": "stop_test" }),
            Err((_, e)) => json!({ "thrown": e }),
        }
    }

    fn adder(app: &App, turn: &Json, link: &mut Link) -> Json {
        let Some(n) = turn["calls"][0]["input"]["n"].as_i64() else {
            if turn["step"] == "start" {
                set(app, |_| 0);
            }
            return state(app);
        };
        let added = link
            .asked(
                POOL_ADD,
                &json!({ "pool": "numbers", "value": n }).to_string(),
            )
            .and_then(|_| link.asked(TARGET, &json!({ "value": n, "label": "n" }).to_string()));
        ended(added, state(app))
    }

    fn consumer(app: &App, turn: &Json, link: &mut Link) -> Json {
        if turn["step"] != "reload" {
            return json!({ "states": {} });
        }
        let generated = link.asked(
            POOL_GENERATE,
            &json!({ "pool": "numbers", "consume": true }).to_string(),
        );
        if let Ok(n) = &generated {
            set(app, |_| match n.parse::<i64>() {
                Ok(n) if n > 5 => -1,
                _ => 0,
            });
        }
        ended(generated, json!({ "states": {} }))
    }

    fn schema(text: &str) -> Json {
        serde_json::from_str(text).unwrap_or_default()
    }

    fn function(name: &str, text: &str) -> Json {
        json!({ "function": ["page", name], "input": schema(text) })
    }

    fn sends() -> Json {
        json!({
            "adapter": "bun",
            "start": [function("clear", NONE)],
            "actions": [function("send", NUMBER)],
            "states": { "page.count": { "type": "integer" } },
        })
    }

    fn reloads() -> Json {
        json!({ "adapter": "other", "actions": [{ "function": ["tab", "reload"], "input": schema(NONE) }] })
    }

    fn two(transport: &str, draw: &str, sends: Json, plays: [Play; 2]) -> Result<Outcome, String> {
        let config = weft_parse::config(&format!(
            "{CONFIG}{transport}\nrun cases=100 steps=50 check=\"sampled\"\n"
        ))
        .map_err(|e| e.to_string())?;
        let d =
            weft_parse::parse(&PAGE.replace("DRAW", draw), &config).map_err(|e| e.to_string())?;
        let at = listen(config.transport.as_ref())?;
        let app = App::default();
        let name = file();
        let runners: Vec<_> = [sends, reloads()]
            .into_iter()
            .zip(plays)
            .map(|(hello, play)| {
                let (at, app, joined) = (at.clone(), app.clone(), name.clone());
                thread::spawn(move || runner(&at, &joined, &hello, &app, play, ["send", "reload"]))
            })
            .collect();
        let outcome = run(&config, &d, Path::new(&name), None, None, &mut |_| {});
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
        o.trace.iter().map(|l| (l.step(), l.input())).collect()
    }

    #[test]
    fn two_runners_on_a_unix_socket_get_each_input_from_weft() -> Result<(), String> {
        let o = two(
            &socket("pass"),
            "",
            sends(),
            [sender, reloader_with_no_change],
        )?;
        assert!(!o.failed);
        assert!(o.tally.iter().all(|t| t.steps > 0), "each verb runs");
        Ok(())
    }

    #[test]
    fn a_failing_case_of_two_runners_shrinks_to_the_least_value_and_steps() -> Result<(), String> {
        let o = two(&socket("shrink"), "", sends(), [sender, reloader])?;
        assert!(o.failed);
        assert!(
            o.origin.as_deref().is_some_and(|origin| {
                origin.starts_with("igte at invariants/page-") && origin.ends_with(".weft.kdl:6:7")
            }),
            "{:?}",
            o.origin
        );
        assert_eq!(
            steps(&o),
            [
                ("start".to_string(), String::new()),
                ("send to=page".to_string(), "n=6".to_string()),
                ("reload to=tab".to_string(), String::new()),
            ]
        );
        Ok(())
    }

    #[test]
    fn a_value_that_one_runner_adds_to_a_pool_is_generated_for_the_other() -> Result<(), String> {
        let o = two(&socket("pool"), "", sends(), [adder, consumer])?;
        assert!(o.failed);
        assert_eq!(
            steps(&o),
            [
                ("start".to_string(), String::new()),
                ("send to=page".to_string(), "n=6".to_string()),
                ("reload to=tab".to_string(), "numbers=6".to_string()),
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
            [sender, reloader],
        )?;
        assert!(!o.failed);
        let o = two(
            &socket("high"),
            "\n    draw {\n      n 7 9\n    }",
            sends(),
            [sender, reloader],
        )?;
        assert_eq!(steps(&o)[1].1, "n=7");
        Ok(())
    }

    #[test]
    fn two_runners_over_http_get_each_input_from_weft() -> Result<(), String> {
        let o = two(
            "transport \"http\" url=\"http://127.0.0.1:7413\"",
            "",
            sends(),
            [sender, reloader_with_no_change],
        )?;
        assert!(!o.failed);
        Ok(())
    }

    #[test]
    fn weft_refuses_a_module_in_the_directory_of_the_diagram() -> Result<(), String> {
        let config = weft_parse::config(&format!(
            "{CONFIG}run cases=50 steps=50 check=\"sampled\"\n"
        ))
        .map_err(|e| e.to_string())?;
        let page = PAGE
            .replace("DRAW", "")
            .replace("../fixtures/page.ts", "./page.ts");
        let error = weft_parse::parse(&page, &config)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("`module \"page\"` is a file in the directory of the diagram"),
            "{error}"
        );
        Ok(())
    }

    #[test]
    fn weft_refuses_a_draw_line_that_names_no_field() {
        let error = two(
            &socket("field"),
            "\n    draw {\n      m 1\n    }",
            sends(),
            [sender, reloader],
        )
        .err()
        .unwrap_or_default();
        assert!(
            error.contains("the line `m` of `draw` names a field of no input"),
            "{error}"
        );
    }
}
