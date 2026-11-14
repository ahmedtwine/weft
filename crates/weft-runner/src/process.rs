use std::env;
use std::fs::{self, File};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use weft_simulator::Session;

const HARNESS: &str = r#"await import(Bun.resolveSync("weft/harness", process.cwd()));
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

fn scratch() -> Result<PathBuf, String> {
    let dir = env::temp_dir().join(format!(
        "weft-{}-{}",
        std::process::id(),
        RUNS.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

pub struct Runner<'a> {
    pub adapter: &'a str,
    pub name: &'a str,
    pub at: &'a str,
    pub env: &'a [(String, String)],
    pub session: Arc<Session>,
}

fn spawn(
    mut command: Command,
    label: String,
    shown: &str,
    dir: PathBuf,
    r: Runner,
) -> Result<JoinHandle<String>, String> {
    let begun = Instant::now();
    let tag = format!("{} · {}", r.name, r.adapter);
    let log = File::create(dir.join("runner.log")).map_err(|e| e.to_string())?;
    let session = r.session;
    let mut child = command
        .envs(r.env.iter().map(|(k, v)| (k, v)))
        .env("WEFT_DIAGRAM", r.name)
        .env("WEFT_TRANSPORT", r.at)
        .env("WEFT_LIB", library()?)
        .env("WEFT_ADAPTER", r.adapter)
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log)
        .spawn()
        .map_err(|e| format!("{label}: {e}"))?;
    eprintln!("▸ {tag} · {shown} · pid {}", child.id());
    Ok(thread::spawn(move || {
        let status = child.wait();
        eprintln!(
            "■ {tag} · {} · {:.2}s",
            status
                .as_ref()
                .map_or("no status".to_string(), |s| s.to_string()),
            begun.elapsed().as_secs_f64()
        );
        let log = fs::read_to_string(dir.join("runner.log")).unwrap_or_default();
        let _ = fs::remove_dir_all(&dir);
        let said = format!(
            "{label} ends with {}: {log}",
            status
                .as_ref()
                .map_or("no status".to_string(), |s| s.to_string())
        );
        if !status.is_ok_and(|s| s.success()) {
            session.lose(&said);
        }
        said
    }))
}

pub fn bun(r: Runner) -> Result<JoinHandle<String>, String> {
    let dir = scratch()?;
    let harness = dir.join("weft.test.ts");
    fs::write(&harness, HARNESS).map_err(|e| format!("{}: {e}", harness.display()))?;
    let mut command = Command::new("bun");
    command
        .arg("test")
        .arg("--timeout")
        .arg("86400000")
        .arg(&harness);
    spawn(command, "bun test".into(), "bun test", dir, r)
}

pub fn command(line: &str, r: Runner) -> Result<JoinHandle<String>, String> {
    let mut command = Command::new("sh");
    command.arg("-c").arg(line);
    let label = format!("runner \"{}\"", r.adapter);
    spawn(command, label, line, scratch()?, r)
}
