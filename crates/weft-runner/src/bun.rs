use std::env;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};

use weft_simulator::Session;

const HARNESS: &str = r#"import { dirname } from "node:path";
await import(Bun.resolveSync("weft/harness", dirname(Bun.env.WEFT_DRIVER ?? "")));
"#;
static RUNS: AtomicU64 = AtomicU64::new(0);

fn library() -> Result<PathBuf, String> {
    let exe = env::current_exe().map_err(|e| e.to_string())?;
    let name = format!("{}weft{}", env::consts::DLL_PREFIX, env::consts::DLL_SUFFIX);
    exe.ancestors()
        .skip(1)
        .take(2)
        .map(|dir| dir.join(&name))
        .find(|path| path.is_file())
        .ok_or_else(|| format!("`{name}` is not beside `{}`", exe.display()))
}

pub fn start(
    diagram: &Path,
    name: &str,
    at: &str,
    session: Arc<Session>,
) -> Result<JoinHandle<String>, String> {
    let driver = diagram.with_extension("ts");
    let driver = driver
        .canonicalize()
        .map_err(|e| format!("{}: {e}", driver.display()))?;
    let dir = env::temp_dir().join(format!(
        "weft-{}-{}",
        std::process::id(),
        RUNS.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let harness = dir.join("weft.test.ts");
    fs::write(&harness, HARNESS).map_err(|e| format!("{}: {e}", harness.display()))?;
    let log = File::create(dir.join("bun.log")).map_err(|e| e.to_string())?;
    let mut child = Command::new("bun")
        .arg("test")
        .arg("--timeout")
        .arg("86400000")
        .arg(&harness)
        .env("WEFT_DRIVER", &driver)
        .env("WEFT_DIAGRAM", name)
        .env("WEFT_TRANSPORT", at)
        .env("WEFT_LIB", library()?)
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log)
        .spawn()
        .map_err(|e| format!("bun test: {e}"))?;
    Ok(thread::spawn(move || {
        let status = child
            .wait()
            .map_or("no status".to_string(), |s| s.to_string());
        let told = fs::read_to_string(dir.join("bun.log")).unwrap_or_default();
        let _ = fs::remove_dir_all(&dir);
        let said = format!("bun test ends with {status}: {told}");
        session.lose(&said);
        said
    }))
}
