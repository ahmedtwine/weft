use std::fs::{self, File};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value as Json, json};

use crate::{Fault, Runner};

const HARNESS: &str = include_str!("bun.test.ts");
const WAIT: Duration = Duration::from_secs(60);
static RUNS: AtomicU64 = AtomicU64::new(0);

pub struct Bun {
    child: Child,
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    schema: Json,
    dir: PathBuf,
}

fn told(dir: &Path) -> String {
    fs::read_to_string(dir.join("bun.log")).unwrap_or_default()
}

impl Bun {
    pub fn start(driver: &Path) -> Result<Bun, String> {
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
        let mut child = Command::new("bun")
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
        let started = Instant::now();
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    if let Ok(Some(status)) = child.try_wait() {
                        return Err(format!(
                            "bun test ends with {status} before it connects: {}",
                            told(&dir)
                        ));
                    }
                    if started.elapsed() > WAIT {
                        let _ = child.kill();
                        return Err(format!(
                            "bun test does not connect in {}s: {}",
                            WAIT.as_secs(),
                            told(&dir)
                        ));
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(e) => return Err(e.to_string()),
            }
        };
        stream.set_nonblocking(false).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(WAIT))
            .map_err(|e| e.to_string())?;
        let writer = stream.try_clone().map_err(|e| e.to_string())?;
        let mut bun = Bun {
            child,
            reader: BufReader::new(stream),
            writer,
            schema: Json::Null,
            dir,
        };
        bun.schema = bun.receive().map_err(|e| match e {
            Fault::Thrown(e) | Fault::Broken(e) => e,
        })?;
        Ok(bun)
    }

    fn receive(&mut self) -> Result<Json, Fault> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => Err(Fault::Broken(format!(
                "bun test closes the socket: {}",
                told(&self.dir)
            ))),
            Ok(_) => serde_json::from_str(&line).map_err(|e| Fault::Broken(format!("{e}: {line}"))),
            Err(e) => Err(Fault::Broken(format!("{e}: {}", told(&self.dir)))),
        }
    }

    fn ask(&mut self, message: Json) -> Result<Json, Fault> {
        writeln!(self.writer, "{message}").map_err(|e| Fault::Broken(e.to_string()))?;
        let answer = self.receive()?;
        match (
            answer.get("state"),
            answer.get("error").and_then(Json::as_str),
        ) {
            (Some(state), None) => Ok(state.clone()),
            (None, Some(error)) => Err(Fault::Thrown(error.to_string())),
            _ => Err(Fault::Broken(format!("bun test answers `{answer}`"))),
        }
    }
}

impl Runner for Bun {
    fn schema(&self) -> &Json {
        &self.schema
    }

    fn reset(&mut self) -> Result<Json, Fault> {
        self.ask(json!({ "reset": true }))
    }

    fn apply(&mut self, object: &str, verb: &str, input: &Json) -> Result<Json, Fault> {
        self.ask(json!({ "apply": verb, "object": object, "input": input }))
    }
}

impl Drop for Bun {
    fn drop(&mut self) {
        let _ = writeln!(self.writer, "{}", json!({ "done": true }));
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.dir);
    }
}
