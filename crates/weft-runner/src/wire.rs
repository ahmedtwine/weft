use std::fs::{self, File};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value as Json, json};
use weft_simulator::{Fault, Runner, Source, field};

const HARNESS: &str = include_str!("bun.test.ts");
const WAIT: Duration = Duration::from_secs(60);
static RUNS: AtomicU64 = AtomicU64::new(0);

enum Line {
    Unix {
        reader: BufReader<UnixStream>,
        writer: UnixStream,
    },
    Http {
        agent: ureq::Agent,
        url: String,
    },
}

impl Line {
    fn unix(stream: UnixStream) -> Result<Line, String> {
        stream.set_nonblocking(false).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(WAIT))
            .map_err(|e| e.to_string())?;
        let writer = stream.try_clone().map_err(|e| e.to_string())?;
        Ok(Line::Unix {
            reader: BufReader::new(stream),
            writer,
        })
    }

    fn receive(&mut self) -> Result<Json, String> {
        match self {
            Line::Unix { reader, .. } => {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => Err("the runner closes the socket".to_string()),
                    Ok(_) => serde_json::from_str(&line).map_err(|e| format!("{e}: {line}")),
                    Err(e) => Err(e.to_string()),
                }
            }
            Line::Http { agent, url } => agent
                .get(url.as_str())
                .call()
                .and_then(|mut r| r.body_mut().read_json())
                .map_err(|e| format!("GET {url}: {e}")),
        }
    }

    fn ask(&mut self, message: &Json) -> Result<Json, String> {
        match self {
            Line::Unix { writer, .. } => {
                writeln!(writer, "{message}").map_err(|e| e.to_string())?;
                self.receive()
            }
            Line::Http { agent, url } => agent
                .post(url.as_str())
                .send_json(message)
                .and_then(|mut r| r.body_mut().read_json())
                .map_err(|e| format!("POST {url}: {e}")),
        }
    }
}

struct Bun {
    child: Child,
    dir: PathBuf,
}

pub struct Wire {
    line: Line,
    schema: Json,
    bun: Option<Bun>,
}

impl Wire {
    fn new(line: Line, bun: Option<Bun>) -> Result<Wire, String> {
        let mut wire = Wire {
            line,
            schema: Json::Null,
            bun,
        };
        let schema = wire.line.receive();
        wire.schema = schema.map_err(|e| wire.told(e))?;
        Ok(wire)
    }

    fn told(&self, e: String) -> String {
        match &self.bun {
            Some(b) => format!(
                "{e}: {}",
                fs::read_to_string(b.dir.join("bun.log")).unwrap_or_default()
            ),
            None => e,
        }
    }

    pub fn unix(path: &str) -> Result<Wire, String> {
        let stream = UnixStream::connect(path).map_err(|e| format!("{path}: {e}"))?;
        Wire::new(Line::unix(stream)?, None)
    }

    pub fn http(url: &str) -> Result<Wire, String> {
        Wire::new(
            Line::Http {
                agent: ureq::Agent::new_with_defaults(),
                url: url.to_string(),
            },
            None,
        )
    }

    pub fn bun(driver: &Path) -> Result<Wire, String> {
        let driver = driver
            .canonicalize()
            .map_err(|e| format!("{}: {e}", driver.display()))?;
        let dir = std::env::temp_dir().join(format!(
            "weft-{}-{}-{}",
            std::process::id(),
            RUNS.fetch_add(1, Ordering::Relaxed),
            driver
                .file_stem()
                .map_or(String::new(), |s| s.to_string_lossy().into_owned())
        ));
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let harness = dir.join("weft.test.ts");
        let socket = dir.join("socket");
        fs::write(&harness, HARNESS).map_err(|e| format!("{}: {e}", harness.display()))?;
        if socket.exists() {
            fs::remove_file(&socket).map_err(|e| format!("{}: {e}", socket.display()))?;
        }
        let listener =
            UnixListener::bind(&socket).map_err(|e| format!("{}: {e}", socket.display()))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let log = File::create(dir.join("bun.log")).map_err(|e| e.to_string())?;
        let child = Command::new("bun")
            .arg("test")
            .arg("--timeout")
            .arg("86400000")
            .arg(&harness)
            .env("WEFT_SOCKET", &socket)
            .env("WEFT_DRIVER", &driver)
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log)
            .spawn()
            .map_err(|e| format!("bun test: {e}"))?;
        let mut bun = Bun { child, dir };
        let started = Instant::now();
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    let ended = bun.child.try_wait().ok().flatten();
                    let late = started.elapsed() > WAIT;
                    if ended.is_some() || late {
                        let _ = bun.child.kill();
                        let log = fs::read_to_string(bun.dir.join("bun.log")).unwrap_or_default();
                        let _ = fs::remove_dir_all(&bun.dir);
                        return Err(match ended {
                            Some(status) => {
                                format!("bun test ends with {status} before it connects: {log}")
                            }
                            None => {
                                format!("bun test does not connect in {}s: {log}", WAIT.as_secs())
                            }
                        });
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(e) => return Err(e.to_string()),
            }
        };
        Wire::new(Line::unix(stream)?, Some(bun))
    }

    fn ask(&mut self, message: Json, source: &dyn Source) -> Result<Json, Fault> {
        let mut message = message;
        loop {
            let answer = self
                .line
                .ask(&message)
                .map_err(|e| Fault::Broken(self.told(e)))?;
            if let Some(score) = answer.get("score") {
                source.target(score.as_f64().ok_or_else(|| {
                    Fault::Broken(format!("the score `{score}` is not a number"))
                })?);
            }
            match (
                answer.get("state"),
                answer.get("error").and_then(Json::as_str),
                answer.get("draw"),
            ) {
                (Some(state), None, None) => return Ok(state.clone()),
                (None, Some(error), None) => return Err(Fault::Thrown(error.to_string())),
                (None, None, Some(draw)) => {
                    let f = field("draw", draw).map_err(Fault::Broken)?;
                    message = json!({ "drawn": source.draw(&f) });
                }
                _ => return Err(Fault::Broken(format!("the runner answers `{answer}`"))),
            }
        }
    }
}

impl Runner for Wire {
    fn schema(&self) -> &Json {
        &self.schema
    }

    fn reset(&mut self, source: &dyn Source) -> Result<Json, Fault> {
        self.ask(json!({ "reset": true }), source)
    }

    fn apply(
        &mut self,
        object: &str,
        verb: &str,
        input: &Json,
        source: &dyn Source,
    ) -> Result<Json, Fault> {
        self.ask(
            json!({ "apply": verb, "object": object, "input": input }),
            source,
        )
    }
}

impl Drop for Wire {
    fn drop(&mut self) {
        if let Line::Unix { writer, .. } = &mut self.line {
            let _ = writeln!(writer, "{}", json!({ "done": true }));
        }
        if let Some(b) = &mut self.bun {
            let _ = b.child.wait();
            let _ = fs::remove_dir_all(&b.dir);
        }
    }
}
