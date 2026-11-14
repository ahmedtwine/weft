mod process;
mod proxy;
mod transport;

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value as Json, json};
use weft_parse::{Adapter, Config, Diagram, Start};
use weft_simulator::{Member, Outcome, simulate};

use transport::{HUB, listen};

const WAIT: Duration = Duration::from_secs(60);

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

pub fn run(
    config: &Config,
    d: &Diagram,
    file: &Path,
    key: Option<&str>,
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
        let line = config
            .runners
            .iter()
            .find(|(adapter, _)| *adapter == i.adapter)
            .map(|(_, start)| start.clone())
            .or_else(|| {
                (i.adapter == "bun" && config.adapter == Adapter::Bun).then_some(Start::Bun)
            });
        let Some(line) = line else {
            continue;
        };
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
            .chain([("WEFT_PROXY".to_string(), proxy.url.clone())])
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
            Start::Command(line) => process::command(&line, r),
            Start::Bun => process::bun(r),
        };
        match runner {
            Ok(runner) => started.push(runner),
            Err(e) => {
                HUB.close(&name);
                return Err(e);
            }
        }
    }
    let joined = session.members(WAIT);
    let outcome = match joined {
        Ok(remotes) => {
            let mut members: Vec<Box<dyn Member>> = remotes
                .into_iter()
                .map(|remote| Box::new(remote) as Box<dyn Member>)
                .collect();
            Ok(simulate(d, &mut members, config.cases, key))
        }
        Err(e) => Err(e),
    };
    HUB.close(&name);
    drop(proxies);
    let logs: Vec<String> = started
        .into_iter()
        .filter_map(|runner| runner.join().ok())
        .collect();
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

    use weft_wire::{DONE, JOIN, Reader, TURN, Writer, receive, send};

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
  module "page" "../src/page.ts"
}
impl "other" {
  module "tab" "../src/tab.ts"
}
"#;
    const NONE: &str = r#"{"type":"object","properties":{},"additionalProperties":false}"#;
    const NUMBER: &str = r#"{"type":"object","properties":{"n":{"type":"integer","minimum":0,"maximum":1000}},"required":["n"],"additionalProperties":false}"#;

    #[derive(Default)]
    struct Page {
        count: i64,
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
        let count = app.lock().map_or(0, |page| page.count);
        json!({ "states": { "page.count": count } })
    }

    fn runner(
        at: &str,
        file: &str,
        hello: Json,
        app: App,
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
        assert_eq!(code[0]["modules"]["page"], "./src/page.ts");
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
            set(app, |n| if n > 5 { -1 } else { n });
        }
        json!({ "states": {} })
    }

    fn reloader_with_no_change(_: &App, _: &Json) -> Json {
        json!({ "states": {} })
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

    fn two(transport: &str, draw: &str, sends: Json, reload: Play) -> Result<Outcome, String> {
        let config = weft_parse::config(&format!(
            "{CONFIG}{transport}\nadapter \"default\" cases=100\n"
        ))
        .map_err(|e| e.to_string())?;
        let d =
            weft_parse::parse(&PAGE.replace("DRAW", draw), &config).map_err(|e| e.to_string())?;
        let at = listen(config.transport.as_ref())?;
        let app = App::default();
        let name = file();
        let runners = [(sends, sender as Play), (reloads(), reload)].map(|(hello, play)| {
            let (at, app, joined) = (at.clone(), app.clone(), name.clone());
            thread::spawn(move || runner(&at, &joined, hello, app, play, ["send", "reload"]))
        });
        let outcome = run(&config, &d, Path::new(&name), None);
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
                ("reload to=tab".to_string(), String::new()),
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
