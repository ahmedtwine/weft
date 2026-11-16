use std::env;
use std::ffi::CStr;
use std::fs::{self, File};
use std::io;
use std::mem::MaybeUninit;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::{Map, Value as Json, json};
use weft_wire::{Given, WEFT};

use crate::hub::Session;
use crate::reaper;

const LIMIT: Duration = Duration::from_secs(10);
const IDLE: Duration = Duration::from_millis(20);

const HARNESS: &str = r#"await import(Bun.resolveSync("weft/harness", process.cwd()));
"#;
static RUNS: AtomicU64 = AtomicU64::new(0);

fn library() -> Result<PathBuf, String> {
    let name = format!("{}weft{}", env::consts::DLL_PREFIX, env::consts::DLL_SUFFIX);
    let here: fn() -> Result<PathBuf, String> = library;
    let mut info = MaybeUninit::<libc::Dl_info>::zeroed();
    if unsafe { libc::dladdr(here as *const libc::c_void, info.as_mut_ptr()) } == 0 {
        return Err(format!("`{name}` is not found"));
    }
    let image = PathBuf::from(
        unsafe { CStr::from_ptr(info.assume_init().dli_fname) }
            .to_string_lossy()
            .into_owned(),
    );
    match image.file_name() == Some(name.as_ref()) {
        true => Ok(image),
        false => image
            .ancestors()
            .skip(1)
            .take(2)
            .map(|dir| dir.join(&name))
            .find(|path| path.is_file())
            .ok_or_else(|| format!("`{name}` is not beside `{}`", image.display())),
    }
}

fn scratch() -> Result<PathBuf, String> {
    let dir = env::temp_dir().join(format!(
        "weft-{}-{}",
        std::process::id(),
        RUNS.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    reaper::path(&dir);
    Ok(dir)
}

pub(crate) struct Runner<'a> {
    pub adapter: &'a str,
    pub name: &'a str,
    pub at: &'a str,
    pub env: &'a [(String, String)],
    pub session: Arc<Session>,
}

pub(crate) struct Started {
    group: u32,
    ended: JoinHandle<String>,
}

fn given(r: &Runner) -> Result<Vec<(String, String)>, String> {
    let given = Given {
        diagram: r.name.to_string(),
        transport: r.at.to_string(),
        lib: library()?.display().to_string(),
        adapter: r.adapter.to_string(),
    };
    let text = serde_json::to_string(&given).map_err(|e| e.to_string())?;
    Ok(r.env
        .iter()
        .cloned()
        .chain([(WEFT.to_string(), text)])
        .collect())
}

fn exited(pid: u32) {
    let mut info = MaybeUninit::<libc::siginfo_t>::zeroed();
    while unsafe {
        libc::waitid(
            libc::P_PID,
            pid,
            info.as_mut_ptr(),
            libc::WEXITED | libc::WNOWAIT,
        )
    } != 0
        && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted
    {}
}

pub(crate) fn ended(started: Vec<Started>) -> Vec<String> {
    let until = Instant::now() + LIMIT;
    let slow = || {
        started
            .iter()
            .filter(|runner| !runner.ended.is_finished())
            .map(|runner| runner.group)
            .collect::<Vec<u32>>()
    };
    while !slow().is_empty() && Instant::now() < until {
        thread::sleep(IDLE);
    }
    reaper::stop(&slow());
    started
        .into_iter()
        .filter_map(|runner| runner.ended.join().ok())
        .collect()
}

pub(crate) fn parent(r: &Runner) -> Result<String, String> {
    let cwd = env::current_dir().map_err(|e| e.to_string())?;
    let env: Map<String, Json> = given(r)?
        .into_iter()
        .map(|(key, value)| (key, Json::String(value)))
        .collect();
    Ok(json!({ "cwd": cwd, "env": env }).to_string())
}

fn spawn(
    mut command: Command,
    label: String,
    shown: &str,
    dir: PathBuf,
    r: Runner,
) -> Result<Started, String> {
    let begun = Instant::now();
    let tag = format!("{} · {}", r.name, r.adapter);
    let log = File::create(dir.join("runner.log")).map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    unsafe {
        command.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        });
    }
    let mut child = command
        .envs(given(&r)?)
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log)
        .process_group(0)
        .spawn()
        .map_err(|e| format!("{label}: {e}"))?;
    let session = r.session;
    let group = child.id();
    reaper::group(group);
    eprintln!("▸ {tag} · {shown} · pid {group}");
    let ended = thread::spawn(move || {
        exited(group);
        reaper::signal(group, libc::SIGKILL);
        let status = child.wait();
        reaper::ended(group);
        eprintln!(
            "■ {tag} · {} · {:.2}s",
            status
                .as_ref()
                .map_or("no status".to_string(), ToString::to_string),
            begun.elapsed().as_secs_f64()
        );
        let log = fs::read_to_string(dir.join("runner.log")).unwrap_or_default();
        let _ = fs::remove_dir_all(&dir);
        let said = format!(
            "{label} ends with {}: {log}",
            status
                .as_ref()
                .map_or("no status".to_string(), ToString::to_string)
        );
        if !status.is_ok_and(|s| s.success()) {
            session.lose(&said);
        }
        said
    });
    Ok(Started { group, ended })
}

pub(crate) fn bun(r: Runner) -> Result<Started, String> {
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

pub(crate) fn command(line: &str, r: Runner) -> Result<Started, String> {
    let mut command = Command::new("sh");
    command.arg("-c").arg(line);
    let label = format!("runner \"{}\"", r.adapter);
    spawn(command, label, line, scratch()?, r)
}
