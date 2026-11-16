mod spans;

use std::{
    env, io, iter,
    path::{Path, PathBuf},
    process::{self, ExitCode},
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use globset::{Glob, GlobSetBuilder};
use ignore::WalkBuilder;
use weft_orchestrator::{CONFIG, Event, Outcome, brief, checks, summary, totals};
use weft_parse::{Config, Diagram};

use spans::{Ended, Spans};

const USAGE: &str = "usage: weft [--ui] [<file>…] | weft --parent <adapter> <file>";
const PARENT: &str = "--parent";
const UI: &str = "--ui";
const ORPHANED: i32 = 2;
const PASS: &str = "pass";
const FAIL: &str = "fail";
const SUFFIX: &str = ".weft.kdl";
const ADDRESS: &str = "127.0.0.1:7357";
const WATCH: Duration = Duration::from_secs(1);
static RAN: Mutex<Option<Option<SystemTime>>> = Mutex::new(None);
static RUNS: AtomicBool = AtomicBool::new(false);

struct Report {
    lines: Vec<String>,
    checks: u64,
}

struct Checked {
    line: String,
    checks: u64,
}

fn files(only: &[String]) -> Result<(Vec<PathBuf>, Config), String> {
    let config = weft_orchestrator::config()?;
    weft_draw::styled(&config.styles).map_err(|e| format!("{CONFIG}: {e}"))?;
    let mut set = GlobSetBuilder::new();
    for pattern in &config.include {
        if !pattern.ends_with(SUFFIX) {
            return Err(format!("{CONFIG}: `{pattern}` does not end in {SUFFIX}"));
        }
        set.add(Glob::new(pattern).map_err(|e| format!("{CONFIG}: {e}"))?);
    }
    let set = set.build().map_err(|e| format!("{CONFIG}: {e}"))?;
    let mut found: Vec<PathBuf> = WalkBuilder::new(".")
        .build()
        .filter_map(Result::ok)
        .map(ignore::DirEntry::into_path)
        .map(|p| p.strip_prefix(".").map(Path::to_path_buf).unwrap_or(p))
        .filter(|p| p.is_file() && set.is_match(p))
        .filter(|p| only.is_empty() || only.iter().any(|named| Path::new(named) == p))
        .collect();
    found.sort();
    match found.is_empty() {
        true => Err(format!(
            "{CONFIG}: no file matches {}",
            config.include.join(", ")
        )),
        false => Ok((found, config)),
    }
}

fn run(
    d: &Diagram,
    file: &Path,
    config: &Config,
    parent: Option<&str>,
    spans: &mut Option<Spans<'_>>,
) -> Result<Outcome, String> {
    weft_orchestrator::tested(config, d, file, parent, &mut |event| {
        if let Event::Joined(line) = event {
            println!("{line}");
        }
        if let Some(spans) = spans {
            spans.hear(event);
        }
    })
}

fn checked(file: &Path, config: &Config, parent: Option<&str>) -> Checked {
    let path = file.strip_prefix(".").unwrap_or(file).display().to_string();
    let started = Instant::now();
    weft_orchestrator::diagram(file, config)
        .and_then(|d| {
            let o = run(&d, file, config, parent, &mut Spans::new(&path, &d))?;
            spans::ended(
                &path,
                Ended {
                    status: match o.failed {
                        true => FAIL,
                        false => PASS,
                    },
                    test_cases: o.runs,
                    steps: o.tally.iter().map(|t| t.steps).sum(),
                    checks: checks(&o),
                    check: o
                        .failed
                        .then(|| o.trace.last().and_then(|l| brief(&l.evidence).pop()))
                        .flatten(),
                    took: started.elapsed(),
                    test_case: o.failed.then_some(o.runs),
                },
            );
            Ok(Checked {
                line: summary(&path, &d, config, &o, started.elapsed()),
                checks: checks(&o),
            })
        })
        .unwrap_or_else(|e| {
            spans::ended(&path, Ended::error(&e));
            Checked {
                line: format!("✕ {path} · {e}"),
                checks: 0,
            }
        })
}

fn report(only: &[String], parent: Option<&str>) -> Result<Report, String> {
    let (files, config) = files(only)?;
    for file in &files {
        let drawn = weft_orchestrator::diagram(file, &config).and_then(|d| {
            let actions = d.actions(file.parent().unwrap_or(Path::new("")))?;
            Ok((d, actions))
        });
        if let Ok((d, actions)) = drawn {
            spans::drawn(&file.display().to_string(), &d, &actions);
        }
    }
    let width = match config.transport {
        Some(_) => 1,
        None => thread::available_parallelism().map_or(1, usize::from),
    };
    let next = AtomicUsize::new(0);
    let mut done: Vec<(usize, Checked)> = thread::scope(|s| {
        (0..width)
            .map(|_| {
                s.spawn(|| {
                    iter::from_fn(|| {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        files.get(i).map(|file| (i, checked(file, &config, parent)))
                    })
                    .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .flat_map(|worker| {
                worker
                    .join()
                    .unwrap_or_else(|p| std::panic::resume_unwind(p))
            })
            .collect()
    });
    done.sort_by_key(|(i, _)| *i);
    Ok(Report {
        checks: done.iter().map(|(_, c)| c.checks).sum(),
        lines: done.into_iter().map(|(_, c)| c.line).collect(),
    })
}

fn changed() -> Option<SystemTime> {
    WalkBuilder::new(".")
        .build()
        .filter_map(Result::ok)
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .max()
}

fn rerun(only: &[String]) {
    let changed = changed();
    let mut ran = RAN.lock().unwrap_or_else(PoisonError::into_inner);
    if *ran == Some(changed) || RUNS.swap(true, Ordering::Relaxed) {
        return;
    }
    *ran = Some(changed);
    weft_dashboard::clear();
    let only = only.to_vec();
    thread::spawn(move || {
        if let Err(e) = report(&only, None) {
            spans::ended(CONFIG, Ended::error(&e));
        }
        RUNS.store(false, Ordering::Relaxed);
    });
}

fn serve(only: &[String]) -> Result<(), String> {
    files(only)?;
    spans::traced(true)?;
    let watched = only.to_vec();
    thread::spawn(move || {
        loop {
            rerun(&watched);
            thread::sleep(WATCH);
        }
    });
    weft_dashboard::serve(ADDRESS, || rerun(only))
}

fn check(only: &[String], parent: Option<&str>) -> Result<bool, String> {
    let started = Instant::now();
    spans::traced(false)?;
    let r = report(only, parent);
    spans::flushed();
    let r = r?;
    println!("{}", totals(&r.lines, r.checks, started.elapsed()));
    Ok(r.lines.iter().any(|l| l.starts_with('✕')))
}

fn joined(adapter: &str, file: &str) -> Result<bool, String> {
    weft_orchestrator::rooted()?;
    thread::spawn(|| {
        let _ = io::copy(&mut io::stdin(), &mut io::sink());
        process::exit(ORPHANED);
    });
    check(&[file.to_string()], Some(adapter))
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args == [weft_orchestrator::REAP] {
        weft_orchestrator::reap();
        return ExitCode::SUCCESS;
    }
    let run = weft_orchestrator::guard().and_then(|()| match args.as_slice() {
        [flag, adapter, file] if flag == PARENT => joined(adapter, file),
        [ui, only @ ..] if ui == UI && only.iter().all(|named| !named.starts_with('-')) => {
            serve(only).map(|_| false)
        }
        only if only.iter().all(|named| !named.starts_with('-')) => check(only, None),
        _ => Err(USAGE.to_string()),
    });
    match run {
        Ok(false) => ExitCode::SUCCESS,
        Ok(true) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}
