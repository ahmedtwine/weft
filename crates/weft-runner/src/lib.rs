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
    Ok(match (&config.transport, &config.adapter) {
        (Some(transport), _) => Box::new(Wire::new(transport::connect(transport)?)?),
        (None, Adapter::Default) => Box::new(Setup::new(d)),
        (None, Adapter::Bun) => Box::new(Wire::new(Box::new(Bun::start(
            &file.with_extension("ts"),
        )?))?),
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
    use weft_simulator::{Outcome, Source};

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
        let listener = UnixListener::bind(&path).unwrap();
        let bound = path.clone();
        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            std::fs::remove_file(bound).unwrap();
            let mut writer = stream.try_clone().unwrap();
            writeln!(writer, "{}", schema()).unwrap();
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
        let mut runner = start(&config, &d, Path::new("page.weft.kdl")).unwrap();
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
        assert!(o.found.is_none());
        assert!(o.tally.iter().all(|t| t.steps > 0 && t.checks == t.steps));
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
        let (_, o) = simulated(&format!("transport unix path=\"{path}\""));
        let trace = o.found.unwrap();
        let rows: Vec<(&str, &str, &str)> = trace
            .iter()
            .map(|l| (l.step.as_str(), l.input.as_str(), l.state.as_str()))
            .collect();
        assert_eq!(rows, [("send to=page", "drawn=4", "page: count = 2")]);
    }

    struct Cut;

    impl Source for Cut {
        fn draw(&self, choice: &weft_simulator::Choice) -> Json {
            choice.draw(self)
        }
        fn bool(&self) -> bool {
            panic!("cut")
        }
        fn int(&self, _: i64, _: i64) -> i64 {
            panic!("cut")
        }
        fn text(&self, _: usize, _: Option<usize>) -> String {
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
        let mut wire =
            Wire::new(transport::connect(&weft_parse::Transport::Unix(path)).unwrap()).unwrap();
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
        assert!(o.found.is_none());
    }
}
