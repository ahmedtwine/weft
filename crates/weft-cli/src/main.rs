mod trace;

use std::{
    env, fs,
    io::{BufRead, BufReader, Write},
    iter,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use globset::{Glob, GlobSetBuilder};
use ignore::WalkBuilder;
use weft_draw::Theme;
use weft_parse::{Config, Diagram, Verdict};
use weft_simulator::{Outcome, simulate, view};

const USAGE: &str = "usage: weft [--ui]";
const CONFIG: &str = "weft.kdl";
const SUFFIX: &str = ".weft.kdl";
const ADDRESS: &str = "127.0.0.1:7357";
static PAGE: Mutex<Option<(Option<SystemTime>, String)>> = Mutex::new(None);

struct Report {
    lines: Vec<String>,
    sections: String,
    theme: Theme,
    checks: u64,
}

struct Checked {
    line: String,
    section: String,
    checks: u64,
}

fn files() -> Result<(Vec<PathBuf>, Config, Theme), String> {
    let source =
        fs::read_to_string(CONFIG).map_err(|_| format!("{CONFIG} is not in this directory"))?;
    let at = |e: String| format!("{CONFIG}: {e}");
    let config = weft_parse::config(&source).map_err(|e| at(e.to_string()))?;
    let theme = Theme::parse(&config.theme).map_err(at)?;
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
        .filter(|p| p.is_file() && set.is_match(p.strip_prefix(".").unwrap_or(p)))
        .collect();
    found.sort();
    match found.is_empty() {
        true => Err(format!(
            "{CONFIG}: no file matches {}",
            config.include.join(", ")
        )),
        false => Ok((found, config, theme)),
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

struct Table {
    head: &'static [&'static str],
    rows: Vec<Vec<String>>,
}

impl Table {
    fn text(&self) -> Vec<String> {
        let line = |cells: Vec<&str>| format!("  {}", cells.join(" · "));
        std::iter::once(line(self.head.to_vec()))
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

fn headline(config: &Config, o: &Outcome, took: Duration) -> String {
    format!(
        "adapter {}{} · {} cases · {} runs · {} steps · {} checks{}{} [{:.2}s]",
        config.adapter,
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

fn tables(o: &Outcome) -> Vec<Table> {
    let trace = Table {
        head: &["step", "action", "input", "state", "check"],
        rows: o
            .trace
            .iter()
            .enumerate()
            .map(|(k, l)| {
                vec![
                    k.to_string(),
                    l.step(),
                    l.input.clone(),
                    view(&l.state),
                    l.evidence.join(" · "),
                ]
            })
            .collect(),
    };
    let tally = Table {
        head: &["action", "steps", "checks"],
        rows: o
            .tally
            .iter()
            .map(|t| vec![t.step.clone(), t.steps.to_string(), t.checks.to_string()])
            .collect(),
    };
    let code = Table {
        head: &["code", "files", "lines"],
        rows: trace::kinds(&o.ran)
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
    };
    [trace, tally, code]
        .into_iter()
        .filter(|t| !t.rows.is_empty())
        .collect()
}

fn summary(path: &str, d: &Diagram, config: &Config, o: &Outcome, took: Duration) -> String {
    let evidence = match d.verdict() {
        Some(Verdict::Fails { evidence, .. }) => evidence,
        _ => Vec::new(),
    };
    let head = format!(
        "{} {path} · {} · {}",
        match o.failed {
            true => '✕',
            false => '✓',
        },
        d.check.as_ref().map_or(String::new(), |c| c.english()),
        evidence.join(" · ")
    );
    [head, format!("  {}", headline(config, o, took))]
        .into_iter()
        .chain(tables(o).iter().flat_map(Table::text))
        .collect::<Vec<_>>()
        .join("\n")
}

fn section(
    path: &str,
    src: &str,
    d: &Diagram,
    o: &Outcome,
    theme: Theme,
) -> Result<String, String> {
    let (words, formula) = d
        .check
        .as_ref()
        .map_or((String::new(), String::new()), |c| {
            (c.english(), c.source())
        });
    Ok(format!(
        r#"<section><h2>{}</h2><small class="text-light">{}</small><div class="s"><div class="d">{}</div>{}</div><details><summary>{}</summary><pre>{}</pre></details></section>"#,
        escape(&words),
        escape(&formula),
        weft_draw::svg(d, theme, &trace::code(&o.ran, path)?)?,
        trace::panel(o)?,
        escape(path),
        escape(src)
    ))
}

fn run(d: &Diagram, file: &Path, config: &Config) -> Result<Outcome, String> {
    let mut runner = weft_runner::start(config, d, file)?;
    let key = fs::canonicalize(file).map_err(|e| e.to_string())?;
    simulate(d, runner.as_mut(), config.cases, key.to_str())
}

fn diagram(file: &Path, config: &Config) -> Result<(String, Diagram), String> {
    let src = fs::read_to_string(file).map_err(|e| e.to_string())?;
    let d = weft_parse::parse(&src, config).map_err(|e| e.to_string())?;
    Ok((src, d))
}

fn checked(file: &Path, config: &Config, theme: Theme, drawn: bool) -> Checked {
    let path = file.strip_prefix(".").unwrap_or(file).display().to_string();
    let started = Instant::now();
    diagram(file, config)
        .and_then(|(src, d)| {
            let o = run(&d, file, config)?;
            Ok(Checked {
                line: summary(&path, &d, config, &o, started.elapsed()),
                section: match drawn {
                    true => section(&path, &src, &d, &o, theme)?,
                    false => String::new(),
                },
                checks: checks(&o),
            })
        })
        .unwrap_or_else(|e| Checked {
            line: format!("✕ {path} · {e}"),
            section: String::new(),
            checks: 0,
        })
}

fn report(drawn: bool) -> Result<Report, String> {
    let (files, config, theme) = files()?;
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
                            .map(|file| (i, checked(file, &config, theme, drawn)))
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
        lines: done.iter().map(|(_, c)| c.line.clone()).collect(),
        sections: done.into_iter().map(|(_, c)| c.section).collect(),
        theme,
    })
}

fn page(r: &Report) -> String {
    let wrong = r.lines.iter().filter(|l| l.starts_with('✕')).count();
    let errors: String = r
        .lines
        .iter()
        .filter(|l| l.starts_with('✕'))
        .map(|l| {
            format!(
                r#"<div role="alert" data-variant="error">{}</div>"#,
                escape(l.lines().next().unwrap_or_default())
            )
        })
        .collect();
    let (scheme, shiki) = match r.theme {
        Theme::Light => ("light", "github-light"),
        Theme::Dark => ("dark", "github-dark"),
    };
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Weft Report</title><link rel="stylesheet" href="https://unpkg.com/@knadh/oat@1.1.0/oat.min.css"><script src="https://unpkg.com/@knadh/oat@1.1.0/oat.min.js" defer></script><style>
:root{{color-scheme:{scheme};--background:{bg}}}
body{{padding:var(--space-6)}}
h1{{font-size:var(--text-3);margin:0}}
h2{{font-size:var(--text-5);margin:0 0 var(--space-2)}}
section{{border-top:1px solid var(--border);padding:var(--space-5) 0}}
.s{{display:flex;gap:var(--space-6);align-items:flex-start;margin:var(--space-4) 0}}
.d{{flex:1 1 0;min-width:0}}
.d svg{{display:block;margin:0 auto}}
aside{{flex:0 0 min(30%,400px);min-width:0;position:sticky;top:var(--space-4);--row:2.25rem;max-height:calc(10*(var(--row) + 1px) + 1px);overflow:auto;scrollbar-width:none}}
aside:has([open]){{max-height:calc(100vh - var(--space-8))}}
aside summary{{box-sizing:border-box;height:var(--row);padding-block:0;font-size:var(--text-7)}}
aside details>:not(summary){{margin:var(--space-2)}}
aside summary svg{{color:var(--success)}}
aside .e summary svg{{color:var(--danger)}}
foreignObject>div{{height:100%}}
figure{{display:flex;flex-direction:column;box-sizing:border-box;height:100%;margin:0;border:1px solid var(--border);border-radius:6px;overflow:hidden;background:var(--background)}}
figcaption{{padding:4px 8px;border-bottom:1px solid var(--border);background:var(--faint);color:var(--muted-foreground);font:10px/14px var(--font-mono);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}}
figure pre{{flex:1;margin:0;padding:5px 0;border:0;border-radius:0;background-color:var(--background)!important;font:11px/15px var(--font-mono);overflow:hidden}}
pre code{{counter-reset:n calc(var(--from) - 1)}}
pre .line::before{{counter-increment:n;content:counter(n);display:inline-block;min-width:4ch;margin-right:2ch;text-align:right;color:var(--faint-foreground)}}
@media(max-width:900px){{.s{{flex-direction:column}}aside{{flex:none;width:100%;position:static;max-height:none}}}}
</style></head><body><h1>Weft Report</h1><p class="text-light">{} · {wrong} wrong</p>{errors}{}<script type="module">
import {{ codeToHtml, bundledLanguages }} from "https://esm.sh/shiki@4.6.0";
for (const pre of document.querySelectorAll("pre[data-lang]")) {{
  const lang = pre.dataset.lang in bundledLanguages ? pre.dataset.lang : "text";
  const box = document.createElement("div");
  box.innerHTML = await codeToHtml(pre.textContent, {{ lang, theme: "{shiki}" }});
  box.firstElementChild.style.setProperty("--from", pre.dataset.from);
  pre.replaceWith(box.firstElementChild);
}}
</script></body></html>"#,
        match r.lines.len() {
            1 => "1 file".to_string(),
            n => format!("{n} files"),
        },
        r.sections,
        bg = r.theme.palette().bg
    )
}

fn changed() -> Option<SystemTime> {
    WalkBuilder::new(".")
        .build()
        .filter_map(Result::ok)
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .max()
}

fn current() -> String {
    let changed = changed();
    let mut last = PAGE.lock().unwrap_or_else(PoisonError::into_inner);
    match last.as_ref() {
        Some((at, page)) if *at == changed => page.clone(),
        _ => {
            let page =
                report(true).map_or_else(|e| format!("<pre>{}</pre>", escape(&e)), |r| page(&r));
            *last = Some((changed, page.clone()));
            page
        }
    }
}

fn respond(mut stream: TcpStream) -> std::io::Result<()> {
    let mut request = String::new();
    BufReader::new(&stream).read_line(&mut request)?;
    let (status, body) = match request.split_whitespace().nth(1) {
        Some("/") => ("200 OK", current()),
        _ => ("404 Not Found", String::new()),
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\ncontent-type: text/html; charset=utf-8\r\ncache-control: no-store\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn serve() -> Result<(), String> {
    files()?;
    let listener = TcpListener::bind(ADDRESS).map_err(|e| format!("{ADDRESS}: {e}"))?;
    println!("Weft Report: http://{ADDRESS}");
    thread::spawn(current);
    for stream in listener.incoming().flatten() {
        std::thread::spawn(|| respond(stream).unwrap_or_else(|e| eprintln!("{e}")));
    }
    Ok(())
}

fn check() -> Result<bool, String> {
    let started = Instant::now();
    let r = report(false)?;
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

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let run = match args.as_slice() {
        [] => check(),
        [ui] if ui == "--ui" => serve().map(|_| false),
        _ => Err(USAGE.to_string()),
    };
    match run {
        Ok(false) => ExitCode::SUCCESS,
        Ok(true) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}
