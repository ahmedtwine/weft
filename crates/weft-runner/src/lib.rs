mod bun;
mod setup;
mod transport;
mod wire;

use std::path::Path;

use weft_parse::{Adapter, Config, Diagram};
use weft_simulator::Runner;

use bun::Bun;
use setup::Setup;
use wire::Wire;

pub fn start<'a>(
    config: &Config,
    d: &'a Diagram,
    file: &Path,
) -> Result<Box<dyn Runner + 'a>, String> {
    let dir = file
        .strip_prefix(".")
        .unwrap_or(file)
        .parent()
        .unwrap_or(Path::new(""));
    Ok(match (&config.transport, &config.adapter) {
        (Some(transport), _) => Box::new(Wire::new(transport::connect(transport)?, dir)?),
        (None, Adapter::Default) => Box::new(Setup::new(d, file)),
        (None, Adapter::Bun) => Box::new(Wire::new(
            Box::new(Bun::start(&file.with_extension("ts"))?),
            dir,
        )?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value as Json, json};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::os::unix::net::UnixListener;
    use std::thread;
    use weft_parse::{Ran, Span};
    use weft_simulator::{Outcome, Source};

    const CODE: &str = "src/page.ts";
    const FILE: &str = "invariants/page.weft.kdl";

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
        let actions = ["send", "reload"].map(|verb| {
            json!({ "type": "object", "weft": verb, "from": [CODE], "run": CODE, "properties": {} })
        });
        json!({
            "state": {
                "type": "object",
                "properties": {
                    "page": {
                        "type": "object",
                        "properties": {
                            "count": { "type": "integer", "weft": "page.count", "from": [CODE] }
                        }
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
        if message.get("ran").is_some() {
            return json!({ "ran": {
                "reset": [{ "file": "page.ts", "from": 3, "to": 5 }],
                "actions": {
                    "send": [{ "file": "page.ts", "from": 7, "to": 9 }],
                    "reload": [{ "file": "page.ts", "from": 11, "to": 11 }]
                }
            } });
        }
        json!({ "state": { "page": { "count": *count } }, "score": *count })
    }

    fn served(reply: impl FnMut(&Json) -> Json + Send + 'static) -> String {
        sent(schema(), reply)
    }

    fn sent(schema: Json, mut reply: impl FnMut(&Json) -> Json + Send + 'static) -> String {
        let path = std::env::temp_dir().join(format!(
            "weft-unix-{}-{:?}",
            std::process::id(),
            thread::current().id()
        ));
        let listener = UnixListener::bind(&path).unwrap();
        let bound = path.clone();
        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            std::fs::remove_file(bound).unwrap();
            let mut writer = stream.try_clone().unwrap();
            writeln!(writer, "{schema}").unwrap();
            for line in BufReader::new(stream).lines() {
                let message: Json = serde_json::from_str(&line.unwrap()).unwrap();
                let out = reply(&message);
                if message.get("done").is_some() {
                    break;
                }
                writeln!(writer, "{out}").unwrap();
            }
        });
        path.display().to_string()
    }

    fn simulated(line: &str) -> (Config, Outcome) {
        let config = weft_parse::config(&format!(
            "include \"**/*.weft.kdl\"\ntheme light\nstyle person icon=user\nstyle browser icon=app-window\n{line}"
        ))
        .unwrap();
        let d = weft_parse::parse(PAGE, &config).unwrap();
        let mut runner = start(&config, &d, Path::new(FILE)).unwrap();
        let o = weft_simulator::simulate(&d, runner.as_mut(), config.cases, None).unwrap();
        (config, o)
    }

    #[test]
    fn the_setup_runs_each_step_and_holds_without_an_adapter_line() {
        let (config, o) = simulated("");
        assert_eq!(
            (config.adapter, config.transport, o.cases, o.runs),
            (Adapter::Default, None, 100, 100)
        );
        assert!(!o.failed);
        assert!(o.tally.iter().all(|t| t.steps > 0 && t.checks == t.steps));
        assert_eq!(
            o.ran,
            Ran {
                reset: vec![span(FILE, 4, 4)],
                actions: vec![
                    ("send".into(), vec![span(FILE, 6, 6)]),
                    ("reload".into(), vec![span(FILE, 9, 9)]),
                ],
            }
        );
    }

    fn span(file: &str, from: usize, to: usize) -> Span {
        Span {
            file: file.into(),
            from,
            to,
        }
    }

    #[test]
    fn a_runner_sends_the_code_of_reset_and_of_each_action_one_time_for_a_run() {
        let mut count = 0;
        let (sent, seen) = std::sync::mpsc::channel();
        let path = served(move |message| {
            sent.send(message.get("ran").is_some()).unwrap();
            answer(&mut count, message)
        });
        let (_, o) = simulated(&format!("transport unix path=\"{path}\""));
        assert_eq!(
            o.ran,
            Ran {
                reset: vec![span("page.ts", 3, 5)],
                actions: vec![
                    ("send".into(), vec![span("page.ts", 7, 9)]),
                    ("reload".into(), vec![span("page.ts", 11, 11)]),
                ],
            }
        );
        assert_eq!(seen.try_iter().filter(|ran| *ran).count(), 1);
    }

    #[test]
    fn a_runner_that_sends_no_code_stops_the_run() {
        let mut count = 0;
        let path = served(move |message| match message.get("ran") {
            Some(_) => json!({ "state": { "page": { "count": count } } }),
            None => answer(&mut count, message),
        });
        let config = weft_parse::config(&format!(
            "include \"**/*.weft.kdl\"\ntheme light\nstyle person icon=user\nstyle browser icon=app-window\ntransport unix path=\"{path}\""
        ))
        .unwrap();
        let d = weft_parse::parse(PAGE, &config).unwrap();
        let mut runner = start(&config, &d, Path::new(FILE)).unwrap();
        let error = weft_simulator::simulate(&d, runner.as_mut(), config.cases, None)
            .err()
            .unwrap();
        assert!(
            error.starts_with("the runner answers `") && error.ends_with("` to `ran`"),
            "{error}"
        );
    }

    #[test]
    fn a_runner_on_a_unix_socket_gets_the_messages_of_the_bun_harness() {
        let mut count = 0;
        let path = served(move |message| answer(&mut count, message));
        let (config, o) = simulated(&format!(
            "adapter bun cases=30\ntransport unix path=\"{path}\""
        ));
        assert_eq!(
            (config.adapter, config.transport),
            (Adapter::Bun, Some(weft_parse::Transport::Unix(path)))
        );
        assert_eq!((o.cases, o.runs), (30, 30));
        assert!(!o.failed);
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
        let (_, o) = simulated(&format!("transport unix path=\"{path}\""));
        assert!(o.failed);
        let rows: Vec<(String, &str, String)> = o
            .trace
            .iter()
            .skip(1)
            .map(|l| (l.step(), l.input.as_str(), weft_simulator::view(&l.state)))
            .collect();
        assert_eq!(
            rows,
            [("send to=page".into(), "drawn=4", "page: count = 2".into())]
        );
    }

    #[test]
    fn a_tag_and_the_function_of_an_action_are_from_a_file_outside_the_driver() {
        let driver = "invariants/page.weft.ts";
        let changed = |path: &str, value: Json| {
            let mut schema = schema();
            *schema.pointer_mut(path).unwrap() = value;
            schema
        };
        let cases = [
            (
                changed("/state/properties/page/properties/count/from", json!([])),
                "the tag `page.count` has no `from`: the runner names the file that registers each tag".to_string(),
            ),
            (
                changed("/state/properties/page/properties/count/from", json!([CODE, driver])),
                format!("the tag `page.count` is from `{driver}`, a file of the driver: only the code outside `invariants` gives it"),
            ),
            (
                changed("/actions/0/from", json!([driver])),
                format!("the tag `send` is from `{driver}`, a file of the driver: only the code outside `invariants` gives it"),
            ),
            (
                changed("/actions/1/run", Json::Null),
                "the function of the action `reload` has no `run`: the runner names the file that exports the function".to_string(),
            ),
            (
                changed("/actions/1/run", json!(driver)),
                format!("the function of the action `reload` is from `{driver}`, a file of the driver: only the code outside `invariants` gives it"),
            ),
        ];
        for (schema, error) in cases {
            let mut count = 0;
            let path = sent(schema, move |message| answer(&mut count, message));
            let config = weft_parse::config(&format!(
                "include \"**/*.weft.kdl\"\ntheme light\nstyle person icon=user\nstyle browser icon=app-window\ntransport unix path=\"{path}\""
            ))
            .unwrap();
            let d = weft_parse::parse(PAGE, &config).unwrap();
            let file = format!("./{FILE}");
            assert_eq!(start(&config, &d, Path::new(&file)).err(), Some(error));
        }
    }

    struct Cut;

    impl Source for Cut {
        fn draw(&self, _: &weft_simulator::Choice) -> Json {
            panic!("cut")
        }
        fn index(&self, _: usize) -> usize {
            panic!("cut")
        }
        fn target(&self, _: f64) {}
    }

    #[test]
    fn a_runner_gets_stop_before_done_when_the_last_case_ends_in_a_draw() {
        let (sent, seen) = std::sync::mpsc::channel();
        let path = served(move |message| {
            sent.send(message.as_object().unwrap().keys().next().cloned().unwrap())
                .unwrap();
            match message.get("apply") {
                Some(_) => json!({ "draw": { "type": "boolean" } }),
                None => json!({ "error": "weft stops the step" }),
            }
        });
        let mut wire = Wire::new(
            transport::connect(&weft_parse::Transport::Unix(path)).unwrap(),
            Path::new(FILE).parent().unwrap(),
        )
        .unwrap();
        let cut = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            wire.apply("page", "send", &json!({}), &Cut)
        }));
        assert!(cut.is_err());
        drop(wire);
        assert_eq!(seen.iter().collect::<Vec<_>>(), ["apply", "stop", "done"]);
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
        let (config, o) = simulated(&format!(
            "adapter default cases=30\ntransport http url=\"{url}\""
        ));
        assert_eq!(
            (config.adapter, config.transport),
            (Adapter::Default, Some(weft_parse::Transport::Http(url)))
        );
        assert_eq!((o.cases, o.runs), (30, 30));
        assert!(!o.failed);
    }
}
