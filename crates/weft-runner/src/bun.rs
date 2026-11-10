use std::fs::{self, File};
use std::io::ErrorKind;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value as Json;

use crate::transport::{Transport, Unix, WAIT};

const HARNESS: &str = include_str!("bun.test.ts");
static RUNS: AtomicU64 = AtomicU64::new(0);

struct Process {
    child: Child,
    dir: PathBuf,
}

impl Process {
    fn told(&self, e: String) -> String {
        format!(
            "{e}: {}",
            fs::read_to_string(self.dir.join("bun.log")).unwrap_or_default()
        )
    }

    fn ended(&mut self) -> Option<ExitStatus> {
        self.child.try_wait().ok().flatten()
    }

    fn accept(&mut self, listener: &UnixListener) -> Result<UnixStream, String> {
        let started = Instant::now();
        loop {
            match (listener.accept(), self.ended()) {
                (Ok((stream, _)), _) => return Ok(stream),
                (Err(e), _) if e.kind() != ErrorKind::WouldBlock => return Err(e.to_string()),
                (Err(_), Some(status)) => {
                    return Err(
                        self.told(format!("bun test ends with {status} before it connects"))
                    );
                }
                (Err(_), None) if started.elapsed() > WAIT => {
                    return Err(
                        self.told(format!("bun test does not connect in {}s", WAIT.as_secs()))
                    );
                }
                (Err(_), None) => thread::sleep(Duration::from_millis(10)),
            }
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.dir);
    }
}

pub struct Bun {
    unix: Unix,
    process: Process,
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
        let mut process = Process { child, dir };
        let stream = process.accept(&listener)?;
        Ok(Bun {
            unix: Unix::new(stream)?,
            process,
        })
    }
}

impl Transport for Bun {
    fn schema(&mut self) -> Result<Json, String> {
        self.unix.schema().map_err(|e| self.process.told(e))
    }

    fn ask(&mut self, message: &Json) -> Result<Json, String> {
        self.unix.ask(message).map_err(|e| self.process.told(e))
    }

    fn done(&mut self) {
        self.unix.done();
        let started = Instant::now();
        while self.process.ended().is_none() && started.elapsed() < WAIT {
            thread::sleep(Duration::from_millis(10));
        }
    }
}
