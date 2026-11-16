use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::os::unix::process::CommandExt;
use std::path::{self, Path};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

pub const REAP: &str = "--reap";
const GRACE: Duration = Duration::from_secs(2);
const IDLE: Duration = Duration::from_millis(20);

static REAPER: Mutex<Option<ChildStdin>> = Mutex::new(None);

pub fn signal(group: u32, signal: i32) -> bool {
    i32::try_from(group).is_ok_and(|group| unsafe { libc::kill(-group, signal) } == 0)
}

pub fn stop(groups: &[u32]) {
    for group in groups {
        signal(*group, libc::SIGTERM);
    }
    let until = Instant::now() + GRACE;
    while Instant::now() < until && groups.iter().any(|group| signal(*group, 0)) {
        thread::sleep(IDLE);
    }
    for group in groups {
        signal(*group, libc::SIGKILL);
    }
}

pub fn guard() -> Result<(), String> {
    let exe = env::current_exe().map_err(|e| e.to_string())?;
    let mut reaper = Command::new(exe)
        .arg(REAP)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("{REAP}: {e}"))?;
    *REAPER.lock().unwrap_or_else(|p| p.into_inner()) = reaper.stdin.take();
    Ok(())
}

fn told(line: String) {
    if let Some(reaper) = REAPER.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
        let _ = writeln!(reaper, "{line}");
    }
}

pub fn group(pid: u32) {
    told(format!("+{pid}"));
}

pub fn ended(pid: u32) {
    told(format!("-{pid}"));
}

pub fn path(path: &Path) {
    if let Ok(path) = path::absolute(path) {
        told(path.display().to_string());
    }
}

pub fn reap() {
    let mut groups = BTreeSet::new();
    let mut paths = Vec::new();
    for line in io::stdin().lock().lines().map_while(Result::ok) {
        match (line.strip_prefix('+'), line.strip_prefix('-')) {
            (Some(group), _) => groups.extend(group.parse::<u32>()),
            (_, Some(group)) => {
                groups.retain(|known| group.parse() != Ok(*known));
            }
            _ => paths.push(line),
        }
    }
    stop(&groups.into_iter().collect::<Vec<_>>());
    for path in paths {
        let _ = fs::remove_dir_all(&path).or_else(|_| fs::remove_file(&path));
    }
}
