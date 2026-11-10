mod setup;
mod wire;

use std::path::Path;

use serde_json::Value as Json;
use weft_parse::{Check, Diagram, State};
use weft_simulator::{Fault, Runner, Source};

pub use setup::Setup;
pub use wire::Wire;

pub enum Adapter<'a> {
    Default(Setup<'a>),
    Wire(Wire),
}

impl<'a> Adapter<'a> {
    pub fn start(
        adapter: &weft_parse::Adapter,
        d: &'a Diagram,
        file: &Path,
    ) -> Result<Adapter<'a>, String> {
        match adapter {
            weft_parse::Adapter::Default => Ok(Adapter::Default(Setup::new(d))),
            weft_parse::Adapter::Bun => Wire::bun(&file.with_extension("ts")).map(Adapter::Wire),
            weft_parse::Adapter::Unix(path) => Wire::unix(path).map(Adapter::Wire),
            weft_parse::Adapter::Http(url) => Wire::http(url).map(Adapter::Wire),
        }
    }

    fn runner(&mut self) -> &mut dyn Runner {
        match self {
            Adapter::Default(s) => s,
            Adapter::Wire(w) => w,
        }
    }
}

impl Runner for Adapter<'_> {
    fn schema(&self) -> &Json {
        match self {
            Adapter::Default(s) => s.schema(),
            Adapter::Wire(w) => w.schema(),
        }
    }

    fn reset(&mut self, source: &dyn Source) -> Result<Json, Fault> {
        self.runner().reset(source)
    }

    fn apply(
        &mut self,
        object: &str,
        verb: &str,
        input: &Json,
        source: &dyn Source,
    ) -> Result<Json, Fault> {
        self.runner().apply(object, verb, input, source)
    }

    fn check(
        &mut self,
        check: &Check,
        cur: &State,
        nxt: &State,
        verb: &str,
    ) -> Result<Option<Vec<String>>, Fault> {
        self.runner().check(check, cur, nxt, verb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::os::unix::net::UnixListener;
    use std::thread;
    use weft_simulator::Outcome;

    const PAGE: &str = r#"
setup {
  person user
  browser page count=0 {
    state count=0 {
      on send count=1
    }
    state count=1 {
      on reload count=1
    }
  }
}
action {
  send from=user to=page
  reload from=user to=page page.count=2
}
check {
  invariant {
    forall browser page {
      and {
        implies {
          in step send
          eq (next)browser.count {
            iadd browser.count 1
          }
        }
        implies {
          in step reload
          eq (next)browser.count browser.count
        }
      }
    }
  }
}
"#;

    fn schema() -> Json {
        let actions = ["send", "reload"]
            .map(|verb| json!({ "type": "object", "weft": verb, "properties": {} }));
        json!({
            "state": {
                "type": "object",
                "properties": {
                    "page": {
                        "type": "object",
                        "properties": { "count": { "type": "integer", "weft": "page.count" } }
                    }
                }
            },
            "actions": actions
        })
    }

    fn answer(count: &mut i64, message: &Json) -> Json {
        match (
            message.get("reset"),
            message.get("apply").and_then(Json::as_str),
        ) {
            (Some(_), _) => *count = 0,
            (_, Some("send")) => *count += 1,
            _ => {}
        }
        json!({ "state": { "page": { "count": *count } }, "score": *count })
    }

    fn served(mut reply: impl FnMut(&Json) -> Json + Send + 'static) -> String {
        let path = std::env::temp_dir().join(format!(
            "weft-unix-{}-{:?}",
            std::process::id(),
            thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            writeln!(writer, "{}", schema()).unwrap();
            for line in BufReader::new(stream).lines() {
                let message: Json = serde_json::from_str(&line.unwrap()).unwrap();
                if message.get("done").is_some() {
                    break;
                }
                writeln!(writer, "{}", reply(&message)).unwrap();
            }
        });
        path.display().to_string()
    }

    fn simulated(line: &str) -> (weft_parse::Adapter, Outcome) {
        let config = weft_parse::config(&format!(
            "include \"**/*.weft.kdl\"\ntheme light\nstyle person icon=user\nstyle browser icon=app-window\n{line}"
        ))
        .unwrap();
        let d = weft_parse::parse(PAGE, &config).unwrap();
        let mut runner = Adapter::start(&config.adapter, &d, Path::new("page.weft.kdl")).unwrap();
        let mut engine = weft_simulator::engine(&config.engine);
        let o = weft_simulator::simulate(&d, &mut runner, engine.as_mut(), config.cases).unwrap();
        (config.adapter.clone(), o)
    }

    #[test]
    fn the_setup_runs_each_step_and_holds_without_an_adapter_line() {
        let (adapter, o) = simulated("");
        assert_eq!(
            (adapter, o.cases, o.runs),
            (weft_parse::Adapter::Default, 100, 100)
        );
        assert!(o.found.is_none());
        assert!(o.tally.iter().all(|t| t.steps > 0 && t.checks == t.steps));
    }

    #[test]
    fn the_setup_holds_under_the_random_engine() {
        let (_, o) = simulated("engine random seed=3");
        assert_eq!((o.cases, o.runs), (100, 100));
        assert!(o.found.is_none());
    }

    #[test]
    fn a_runner_on_a_unix_socket_gets_the_messages_of_the_bun_harness() {
        let mut count = 0;
        let path = served(move |message| answer(&mut count, message));
        let (adapter, o) = simulated(&format!("adapter unix path=\"{path}\" cases=30"));
        assert_eq!(adapter, weft_parse::Adapter::Unix(path));
        assert_eq!((o.cases, o.runs), (30, 30));
        assert!(o.found.is_none());
    }

    #[test]
    fn hegel_shrinks_a_value_that_the_runner_draws_in_a_step() {
        let mut count = 0;
        let mut waiting = false;
        let path = served(move |message| {
            let drawn = message.get("drawn").and_then(Json::as_i64);
            let stop = message.get("stop").is_some();
            let send = message.get("apply").and_then(Json::as_str) == Some("send");
            match (waiting, drawn, stop, send) {
                (true, Some(n), _, _) => {
                    waiting = false;
                    count += if n >= 4 { 2 } else { 1 };
                    json!({ "state": { "page": { "count": count } } })
                }
                (true, None, true, _) => {
                    waiting = false;
                    json!({ "error": "weft stops the step" })
                }
                (true, None, false, _) => json!({ "error": format!("{message} is not drawn") }),
                (false, _, _, true) => {
                    waiting = true;
                    json!({ "draw": { "type": "integer", "minimum": 0, "maximum": 9 } })
                }
                (false, _, _, false) => answer(&mut count, message),
            }
        });
        let (_, o) = simulated(&format!("adapter unix path=\"{path}\" cases=100"));
        let trace = o.found.unwrap();
        let rows: Vec<(&str, &str, &str)> = trace
            .iter()
            .map(|l| (l.step.as_str(), l.input.as_str(), l.state.as_str()))
            .collect();
        assert_eq!(rows, [("send to=page", "drawn=4", "page: count = 2")]);
    }

    #[test]
    fn a_runner_over_http_gives_the_schema_on_get_and_each_state_on_post() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/weft", listener.local_addr().unwrap());
        thread::spawn(move || {
            let mut count = 0;
            for stream in listener.incoming() {
                let mut reader = BufReader::new(stream.unwrap());
                let mut head = Vec::new();
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap() > 2 {
                    head.push(std::mem::take(&mut line).to_lowercase());
                }
                let size = head
                    .iter()
                    .find_map(|h| h.strip_prefix("content-length:"))
                    .map_or(0, |n| n.trim().parse().unwrap());
                let mut body = vec![0; size];
                reader.read_exact(&mut body).unwrap();
                let out = match head[0].starts_with("get") {
                    true => schema(),
                    false => answer(&mut count, &serde_json::from_slice(&body).unwrap()),
                }
                .to_string();
                write!(
                    reader.get_mut(),
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{out}",
                    out.len()
                )
                .unwrap();
            }
        });
        let (adapter, o) = simulated(&format!("adapter http url=\"{url}\" cases=30"));
        assert_eq!(adapter, weft_parse::Adapter::Http(url));
        assert_eq!((o.cases, o.runs), (30, 30));
        assert!(o.found.is_none());
    }
}
