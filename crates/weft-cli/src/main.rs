mod spans;
mod trace;

use std::{
    env, fs,
    io,
    iter,
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
use weft_parse::{Config, Diagram};
use weft_simulator::{Outcome, Tally, view};

use spans::{Ended, Spans};

const USAGE: &str = "usage: weft [--ui] [<file>…] | weft --parent <adapter> <file>";
const PARENT: &str = "--parent";
const UI: &str = "--ui";
const ORPHANED: i32 = 2;
const PASS: &str = "pass";
const FAIL: &str = "fail";
const CONFIG: &str = "weft.kdl";
const SUFFIX: &str = ".weft.kdl";
const ADDRESS: &str = "127.0.0.1:7357";
const INPUT: usize = 80;
const DRAWN: &str = "* draw ";
const SHARE: &str = " of test cases";
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
    let source =
        fs::read_to_string(CONFIG).map_err(|_| format!("{CONFIG} is not in this directory"))?;
    let at = |e: String| format!("{CONFIG}: {e}");
    let config = weft_parse::config(&source).map_err(|e| at(e.to_string()))?;
    weft_draw::styled(&config.styles).map_err(at)?;
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
        .map(|e| e.into_path())
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

struct Table {
    head: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Table {
    fn new<const N: usize>(head: [&str; N], rows: Vec<Vec<String>>) -> Table {
        Table {
            head: head.map(String::from).to_vec(),
            rows,
        }
    }

    fn text(&self) -> Vec<String> {
        let line = |cells: Vec<&str>| format!("  {}", cells.join(" · "));
        std::iter::once(line(self.head.iter().map(String::as_str).collect()))
            .chain(self.rows.iter().map(|cells| {
                line(
                    cells
                        .iter()
                        .map(|c| match c.is_empty() {
                            true => "-",
                            false => c,
                        })
                        .collect(),
                )
            }))
            .collect()
    }
}

fn checks(o: &Outcome) -> u64 {
    o.tally.iter().map(|t| t.checks).sum()
}

fn headline(d: &Diagram, config: &Config, o: &Outcome, took: Duration) -> String {
    format!(
        "{}{} · {} cases · {} runs · {} steps · {} checks{}{} [{:.2}s]",
        d.impls
            .iter()
            .map(|i| i.adapter.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        config
            .transport
            .as_ref()
            .map_or(String::new(), |t| format!(" · transport {t}")),
        o.cases,
        o.runs,
        o.tally.iter().map(|t| t.steps).sum::<u64>(),
        checks(o),
        match (o.failed, o.trace.len() - 1) {
            (false, _) => String::new(),
            (true, 0) => " · fails at start".to_string(),
            (true, 1) => " · fails after 1 step".to_string(),
            (true, n) => format!(" · fails after {n} steps"),
        },
        o.caveat
            .as_ref()
            .map_or(String::new(), |c| format!(" · {c}")),
        took.as_secs_f64()
    )
}

fn tables(d: &Diagram, o: &Outcome) -> Vec<Table> {
    let named = trace::checked(d);
    let shown = |state: &weft_parse::State| {
        let kept: weft_parse::State = state
            .iter()
            .map(|(object, pairs)| {
                (
                    object.clone(),
                    pairs
                        .iter()
                        .filter(|(key, _)| {
                            named.is_empty() || named.contains(&(object.clone(), key.to_string()))
                        })
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                )
            })
            .filter(|(_, pairs): &(String, weft_parse::Pairs)| !pairs.is_empty())
            .collect();
        view(&kept)
    };
    let cut = |text: String| {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        match text.chars().count() > INPUT {
            true => text.chars().take(INPUT).chain(['…']).collect(),
            false => text,
        }
    };
    let trace = Table::new(
        ["step", "action", "input", "state", "check"],
        o.trace
            .iter()
            .enumerate()
            .flat_map(|(k, l)| {
                iter::once(vec![
                    k.to_string(),
                    l.step(),
                    cut(l.input()),
                    shown(&l.state),
                    trace::brief(&l.evidence).join(" · "),
                ])
                .chain(trace::sorted(&l.hops).into_iter().map(|h| {
                    vec![
                        String::new(),
                        format!("↳ {} → {} {} {}", h.from, h.to, h.method, h.path),
                        cut(trace::compact(&h.body)),
                        cut(format!("{} {}", h.status, trace::compact(&h.reply))),
                        String::new(),
                    ]
                }))
            })
            .collect(),
    );
    let case = Table::new(
        ["case", "runs"],
        o.counts
            .statuses
            .iter()
            .map(|(status, runs)| vec![status.to_string(), runs.to_string()])
            .collect(),
    );
    let tally = Table::new(
        ["action", "runs", "steps", "refused", "checks"],
        o.tally
            .iter()
            .map(|t| {
                vec![
                    t.step.clone(),
                    t.runs.to_string(),
                    t.steps.to_string(),
                    t.refused
                        .iter()
                        .map(|(cause, tries)| format!("{cause} {tries}"))
                        .collect::<Vec<_>>()
                        .join(", "),
                    t.checks.to_string(),
                ]
            })
            .collect(),
    );
    let verb = |t: &Tally| t.step.split(' ').next().unwrap_or_default().to_string();
    let then = Table {
        head: iter::once("then".to_string())
            .chain(o.tally.iter().map(verb))
            .collect(),
        rows: o
            .tally
            .iter()
            .filter(|_| o.tally.len() > 1)
            .map(|t| {
                iter::once(verb(t))
                    .chain(t.then.iter().map(u64::to_string))
                    .collect()
            })
            .collect(),
    };
    let call = Table::new(
        ["call", "status", "count"],
        o.counts
            .calls
            .iter()
            .map(|((call, status), count)| {
                vec![call.clone(), status.to_string(), count.to_string()]
            })
            .collect(),
    );
    let pool = Table::new(
        ["pool", "add", "generate", "consume", "empty"],
        o.counts
            .pools
            .iter()
            .map(|(pool, p)| {
                iter::once(pool.clone())
                    .chain([p.add, p.generate, p.consume, p.empty].map(|n| n.to_string()))
                    .collect()
            })
            .collect(),
    );
    let invariant = Table::new(
        ["invariant", "checks", "items"],
        o.counts
            .invariants
            .iter()
            .map(|i| {
                vec![
                    i.name.clone(),
                    i.checks.to_string(),
                    i.items.map_or(String::new(), |items| items.to_string()),
                ]
            })
            .collect(),
    );
    let code = Table::new(
        ["code", "files", "lines"],
        trace::kinds(&o.ran)
            .iter()
            .map(|(name, spans)| {
                let mut files: Vec<&String> = spans.iter().map(|s| &s.file).collect();
                files.sort();
                files.dedup();
                vec![
                    name.to_string(),
                    files.len().to_string(),
                    spans.iter().map(trace::size).sum::<usize>().to_string(),
                ]
            })
            .collect(),
    );
    let drawn = Table::new(
        ["draw", "cases"],
        o.output
            .iter()
            .filter_map(|line| line.trim().strip_prefix(DRAWN)?.rsplit_once(": "))
            .map(|(value, share)| {
                vec![value.to_string(), share.trim_end_matches(SHARE).to_string()]
            })
            .collect(),
    );
    [
        trace, case, tally, then, call, pool, invariant, code, drawn,
    ]
        .into_iter()
        .filter(|t| !t.rows.is_empty())
        .collect()
}

fn invariants(d: &Diagram) -> Vec<String> {
    d.objects
        .iter()
        .flat_map(|o| {
            o.invariants
                .iter()
                .map(|invariant| format!("{} · {}", o.name, invariant.source()))
        })
        .collect()
}

fn summary(path: &str, d: &Diagram, config: &Config, o: &Outcome, took: Duration) -> String {
    let head = format!(
        "{} {path}{}",
        match o.failed {
            true => '✕',
            false => '✓',
        },
        match (o.failed, o.trace.last()) {
            (true, Some(last)) => format!(" · {}", trace::brief(&last.evidence).join(" · ")),
            _ => String::new(),
        }
    );
    [head, format!("  {}", headline(d, config, o, took))]
        .into_iter()
        .chain(invariants(d).into_iter().map(|rule| format!("  {rule}")))
        .chain(tables(d, o).iter().flat_map(Table::text))
        .collect::<Vec<_>>()
        .join("\n")
}

fn run(
    d: &Diagram,
    file: &Path,
    config: &Config,
    parent: Option<&str>,
    spans: &mut Option<Spans<'_>>,
) -> Result<Outcome, String> {
    let key = fs::canonicalize(file).map_err(|e| e.to_string())?;
    weft_runner::run(config, d, file, key.to_str(), parent, &mut |event| {
        if let Some(spans) = spans {
            spans.hear(event);
        }
    })
}

fn diagram(file: &Path, config: &Config) -> Result<Diagram, String> {
    let src = fs::read_to_string(file).map_err(|e| e.to_string())?;
    weft_parse::parse(&src, config).map_err(|e| e.to_string())
}

fn checked(file: &Path, config: &Config, parent: Option<&str>) -> Checked {
    let path = file.strip_prefix(".").unwrap_or(file).display().to_string();
    let started = Instant::now();
    diagram(file, config)
        .and_then(|d| {
            let o = run(&d, file, config, parent, &mut Spans::new(&path, &d))?;
            spans::ended(&path, Ended {
                status: match o.failed {
                    true => FAIL,
                    false => PASS,
                },
                test_cases: o.runs,
                steps: o.tally.iter().map(|t| t.steps).sum(),
                checks: checks(&o),
                check: o
                    .failed
                    .then(|| o.trace.last().and_then(|l| trace::brief(&l.evidence).pop()))
                    .flatten(),
                took: started.elapsed(),
                test_case: o.failed.then_some(o.runs),
            });
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
                        files
                            .get(i)
                            .map(|file| (i, checked(file, &config, parent)))
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
    weft_dashboard::serve(ADDRESS, || rerun(only))
}

fn check(only: &[String], parent: Option<&str>) -> Result<bool, String> {
    let started = Instant::now();
    spans::traced(false)?;
    let r = report(only, parent);
    spans::flushed();
    let r = r?;
    r.lines.iter().for_each(|l| println!("{l}"));
    let fail = r.lines.iter().filter(|l| l.starts_with('✕')).count();
    println!(
        "\n {} pass\n {fail} fail\n {} checks\nRan {} in {:.2}s",
        r.lines.len() - fail,
        r.checks,
        match r.lines.len() {
            1 => "1 file".to_string(),
            n => format!("{n} files"),
        },
        started.elapsed().as_secs_f64()
    );
    Ok(fail > 0)
}

fn joined(adapter: &str, file: &str) -> Result<bool, String> {
    let cwd = env::current_dir().map_err(|e| e.to_string())?;
    let root = cwd
        .ancestors()
        .find(|dir| dir.join(CONFIG).is_file())
        .ok_or_else(|| format!("{CONFIG} is not in this directory or in a directory above it"))?;
    env::set_current_dir(root).map_err(|e| e.to_string())?;
    thread::spawn(|| {
        let _ = io::copy(&mut io::stdin(), &mut io::sink());
        process::exit(ORPHANED);
    });
    check(&[file.to_string()], Some(adapter))
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args == [weft_runner::REAP] {
        weft_runner::reap();
        return ExitCode::SUCCESS;
    }
    let run = weft_runner::guard().and_then(|()| match args.as_slice() {
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
