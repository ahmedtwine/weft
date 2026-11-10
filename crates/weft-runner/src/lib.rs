mod bun;
mod setup;
mod transport;

use std::path::Path;
use std::time::Duration;

use weft_parse::{Adapter, Config, Diagram};
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
    let name = file.display().to_string();
    let at = listen(config.transport.as_ref())?;
    let session = HUB.open(&name, file.parent().unwrap_or(Path::new("")));
    let bun = match config.adapter {
        Adapter::Bun => Some(bun::start(file, &name, &at, session.clone())?),
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
    let told = bun.and_then(|bun| bun.join().ok());
    match (outcome, told) {
        (Err(e), Some(told)) => Err(format!("{e}: {told}")),
        (outcome, _) => outcome,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::os::unix::net::UnixStream;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Instant;

    use weft_simulator::{CALLS, PRINT};
    use weft_wire::{DONE, JOIN, Reader, TURN, Writer, receive, send};

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
  reload from=user to=page page.count=0
}
check {
  invariant {
    forall browser page {
      implies {
        in step reload
        eq (next)browser.count browser.count
      }
    }
  }
}
"#;
    const VALID: u32 = 0;
    const OVERRUN: u32 = 2;
    const INTERESTING: u32 = 3;
    const STOP: &str = "-1:";
    const MOST: i64 = 1000;

    type App = Arc<Mutex<i64>>;

    enum Link {
        Unix(UnixStream),
        Http(String),
    }

    struct Runner {
        link: Link,
        pending: Vec<Writer>,
    }

    fn index(name: &str) -> u16 {
        CALLS
            .iter()
            .position(|c| *c == name)
            .map_or(u16::MAX, |i| i as u16)
    }

    fn call(index: u16) -> Writer {
        let mut w = Writer::default();
        w.put(index);
        w
    }

    impl Runner {
        fn ask(&mut self, last: Writer) -> Result<Vec<u8>, String> {
            let mut message = Writer::default();
            message.put(PRINT);
            message.put(self.pending.len() as u32 + 1);
            for w in self.pending.drain(..).chain([last]) {
                message.0.extend(w.0);
            }
            let reply = match &mut self.link {
                Link::Unix(stream) => send(stream, &message.0)
                    .and_then(|()| receive(stream))
                    .map_err(|e| e.to_string())?,
                Link::Http(url) => ureq_post(url, &message.0)?,
            };
            let mut r = Reader(&reply);
            match r.get::<i32>().map_err(|_| "short")? {
                0 => Ok(r.0.to_vec()),
                code => Err(format!(
                    "{code}: {}",
                    String::from_utf8_lossy(r.bytes().map_err(|_| "short")?)
                )),
            }
        }

        fn hegel(&mut self, name: &str, args: impl FnOnce(&mut Writer)) -> Result<Vec<u8>, String> {
            let mut w = call(index(name));
            args(&mut w);
            self.ask(w)
        }

        fn said(&mut self, call: u16, parts: &[&str]) -> Result<Vec<u8>, String> {
            let mut w = self::call(call);
            parts.iter().for_each(|part| w.bytes(part.as_bytes()));
            self.ask(w)
        }

        fn turn(&mut self, report: &str) -> Result<String, String> {
            let reply = self.said(TURN, &[report])?;
            Ok(String::from_utf8_lossy(Reader(&reply).bytes().map_err(|_| "short")?).into_owned())
        }
    }

    fn ureq_post(url: &str, body: &[u8]) -> Result<Vec<u8>, String> {
        let host = url.trim_start_matches("http://");
        let (address, path) = host.split_once('/').unwrap_or((host, ""));
        let mut stream = std::net::TcpStream::connect(address).map_err(|e| e.to_string())?;
        use std::io::{Read, Write};
        write!(
            stream,
            "POST /{path} HTTP/1.1\r\nhost: {address}\r\ncontent-length: {}\r\n\r\n",
            body.len()
        )
        .and_then(|()| stream.write_all(body))
        .map_err(|e| e.to_string())?;
        let mut reader = std::io::BufReader::new(stream);
        let mut size = 0;
        loop {
            let mut line = String::new();
            std::io::BufRead::read_line(&mut reader, &mut line).map_err(|e| e.to_string())?;
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

    fn state(app: &App) -> String {
        format!(r#"{{"page":{{"count":{}}}}}"#, app.lock().map_or(0, |n| *n))
    }

    fn runner(
        at: &str,
        hello: &str,
        app: App,
        play: fn(&mut Runner, u64, &App, &str) -> Result<String, String>,
    ) -> Result<(), String> {
        let link = match at.strip_prefix("unix:") {
            Some(path) => Link::Unix(UnixStream::connect(path).map_err(|e| e.to_string())?),
            None => Link::Http(format!("{at}/{}", hello.len())),
        };
        let mut r = Runner {
            link,
            pending: Vec::new(),
        };
        r.said(JOIN, &[FILE, hello])?;
        let run = Reader(&r.hegel("hegel_run_start", |w| {
            w.put(0u64);
            w.put(false);
        })?)
        .get::<u64>()
        .map_err(|_| "short")?;
        loop {
            let tc = Reader(&r.hegel("hegel_next_test_case", |w| w.put(run))?)
                .get::<u64>()
                .map_err(|_| "short")?;
            if tc == 0 {
                break;
            }
            let mut now = r.turn("{}")?;
            let status = loop {
                match (now.contains("\"done\""), now.contains("\"failed\"")) {
                    (true, _) => break VALID,
                    (_, true) => break INTERESTING,
                    _ => match play(&mut r, tc, &app, &now) {
                        Ok(next) => now = next,
                        Err(e) if e.starts_with(STOP) => break OVERRUN,
                        Err(e) => return Err(e),
                    },
                }
            };
            let mut w = call(index("hegel_mark_complete"));
            w.put(tc);
            w.put(status);
            w.opt((status == INTERESTING).then_some(&b"runner"[..]));
            r.pending.push(w);
        }
        r.hegel("hegel_run_result", |w| w.put(run))?;
        let verb = if hello.contains("\"send\"") {
            "send"
        } else {
            "reload"
        };
        r.said(
            DONE,
            &[&format!(
                r#"{{"ran":{{"reset":[{{"file":"src/page.ts","from":1,"to":2}}],"actions":{{"{verb}":[{{"file":"src/page.ts","from":3,"to":4}}]}}}}}}"#
            )],
        )?;
        Ok(())
    }

    fn sender(r: &mut Runner, tc: u64, app: &App, now: &str) -> Result<String, String> {
        let report = match now {
            n if n.contains("\"reset\"") => {
                *app.lock().map_err(|e| e.to_string())? = 0;
                format!(r#"{{"state":{}}}"#, state(app))
            }
            n if n.contains("\"run\"") => {
                let drawn = r.hegel("hegel_generate_integer", |w| {
                    w.put(tc);
                    w.put(0i64);
                    w.put(MOST);
                })?;
                let n = Reader(&drawn).get::<i64>().map_err(|_| "short")?;
                *app.lock().map_err(|e| e.to_string())? = n;
                format!(r#"{{"state":{},"input":"n={n}"}}"#, state(app))
            }
            _ => format!(r#"{{"state":{}}}"#, state(app)),
        };
        r.turn(&report)
    }

    fn reloader(r: &mut Runner, _: u64, app: &App, now: &str) -> Result<String, String> {
        let report = match now.contains("\"run\"") {
            true => {
                let mut n = app.lock().map_err(|e| e.to_string())?;
                if *n > 5 {
                    *n = 0;
                }
                drop(n);
                r#"{"state":{},"input":""}"#.to_string()
            }
            false => r#"{"state":{}}"#.to_string(),
        };
        r.turn(&report)
    }

    fn reloader_that_keeps(r: &mut Runner, _: u64, _: &App, now: &str) -> Result<String, String> {
        r.turn(match now.contains("\"run\"") {
            true => r#"{"state":{},"input":""}"#,
            false => r#"{"state":{}}"#,
        })
    }

    const SENDS: &str = r#"{"reads":[{"tag":"page.count","from":["src/page.ts"]}],"actions":[{"tag":"send","from":["src/page.ts"],"run":["src/page.ts"]}],"mocks":[]}"#;
    const RELOADS: &str = r#"{"reads":[],"actions":[{"tag":"reload","from":["src/page.ts"],"run":["src/page.ts"]}],"mocks":[]}"#;

    fn two(
        transport: &str,
        reload: fn(&mut Runner, u64, &App, &str) -> Result<String, String>,
    ) -> Result<Outcome, String> {
        let config = weft_parse::config(&format!(
            "include \"**/*.weft.kdl\"\ntheme light\nstyle person icon=user\nstyle browser icon=app-window\n{transport}\nadapter default cases=100\n"
        ))
        .map_err(|e| e.to_string())?;
        let d = weft_parse::parse(PAGE, &config).map_err(|e| e.to_string())?;
        let at = listen(config.transport.as_ref())?;
        let app = App::default();
        let runners = [
            (
                SENDS,
                sender as fn(&mut Runner, u64, &App, &str) -> Result<String, String>,
            ),
            (RELOADS, reload),
        ]
        .map(|(hello, play)| {
            let (at, app) = (at.clone(), app.clone());
            thread::spawn(move || runner(&at, hello, app, play))
        });
        let outcome = run(&config, &d, Path::new(FILE), None);
        for r in runners {
            r.join().map_err(|_| "a runner panics")??;
        }
        outcome
    }

    fn socket(name: &str) -> String {
        format!(
            "transport unix path=\"{}\"",
            env::temp_dir()
                .join(format!("weft-{name}-{}.sock", std::process::id()))
                .display()
        )
    }

    #[test]
    fn two_runners_on_a_unix_socket_share_each_case_and_hold() -> Result<(), String> {
        let o = two(&socket("hold"), reloader_that_keeps)?;
        assert!(!o.failed);
        assert!(o.tally.iter().all(|t| t.steps > 0), "each verb runs");
        Ok(())
    }

    #[test]
    fn a_failing_case_of_two_runners_shrinks_to_the_least_value_and_steps() -> Result<(), String> {
        let started = Instant::now();
        let o = two(&socket("shrink"), reloader)?;
        assert!(o.failed);
        let steps: Vec<(String, String)> = o
            .trace
            .iter()
            .map(|l| (l.step(), l.input.clone()))
            .collect();
        assert_eq!(
            steps,
            [
                ("start".to_string(), String::new()),
                ("send to=page".to_string(), "n=6".to_string()),
                ("reload to=page".to_string(), String::new()),
            ],
            "after {:?}",
            started.elapsed()
        );
        Ok(())
    }

    #[test]
    fn two_runners_over_http_share_each_case_and_hold() -> Result<(), String> {
        let o = two(
            "transport http url=\"http://127.0.0.1:7413\"",
            reloader_that_keeps,
        )?;
        assert!(!o.failed);
        Ok(())
    }

    #[test]
    fn the_setup_adapter_runs_the_diagram_without_the_code() -> Result<(), String> {
        let config = weft_parse::config("include \"**/*.weft.kdl\"\ntheme light\nstyle person icon=user\nstyle browser icon=app-window\nadapter default cases=50\n")
            .map_err(|e| e.to_string())?;
        let d = weft_parse::parse(PAGE, &config).map_err(|e| e.to_string())?;
        let o = run(&config, &d, Path::new(FILE), None)?;
        assert!(!o.failed);
        assert_eq!(o.cases, 50);
        Ok(())
    }

    #[test]
    fn weft_refuses_a_tag_that_a_file_of_the_driver_registers() {
        let hello = SENDS.replace("[\"src/page.ts\"]", "[\"invariants/page.weft.ts\"]");
        let config = weft_parse::config(&format!(
            "include \"**/*.weft.kdl\"\ntheme light\nstyle person icon=user\nstyle browser icon=app-window\n{}\nadapter default cases=10\n",
            socket("refuse")
        ))
        .unwrap_or_else(|e| panic!("{e}"));
        let d = weft_parse::parse(PAGE, &config).unwrap_or_else(|e| panic!("{e}"));
        let at = listen(config.transport.as_ref()).unwrap_or_else(|e| panic!("{e}"));
        let joined = thread::spawn(move || {
            runner(
                &at,
                Box::leak(hello.into_boxed_str()),
                App::default(),
                sender,
            )
        });
        let error = run(&config, &d, Path::new(FILE), None)
            .err()
            .unwrap_or_default();
        let _ = joined.join();
        assert!(
            error.contains(
                "the tag `page.count` is from `invariants/page.weft.ts`, a file of the driver"
            ),
            "{error}"
        );
    }
}
