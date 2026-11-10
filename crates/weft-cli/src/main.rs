use std::{
    env, fs,
    io::{BufRead, BufReader, Write},
    iter,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};

use globset::{Glob, GlobSetBuilder};
use ignore::WalkBuilder;
use weft_draw::Theme;
use weft_parse::{Config, Diagram, Verdict};
use weft_simulator::{Outcome, simulate};

const USAGE: &str = "usage: weft [--ui]";
const CONFIG: &str = "weft.kdl";
const SUFFIX: &str = ".weft.kdl";
const ADDRESS: &str = "127.0.0.1:7357";
const OPEN: &str = match cfg!(target_os = "macos") {
    true => "open",
    false => "xdg-open",
};

struct Report {
    lines: Vec<String>,
    sections: String,
    theme: Theme,
}

impl Report {
    fn failed(&self) -> bool {
        self.lines.iter().any(|l| l.starts_with('✕'))
    }
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

fn headline(config: &Config, o: &Outcome) -> String {
    format!(
        "adapter {}{} · {} cases · {} runs · {}{}",
        config.adapter,
        config
            .transport
            .as_ref()
            .map_or(String::new(), |t| format!(" · transport {t}")),
        o.cases,
        o.runs,
        o.found
            .as_ref()
            .map_or("holds".to_string(), |t| match t.len() {
                1 => "fails after 1 step".to_string(),
                n => format!("fails after {n} steps"),
            }),
        o.caveat
            .as_ref()
            .map_or(String::new(), |c| format!(" · {c}"))
    )
}

fn tables(o: &Outcome) -> Vec<Table> {
    let trace = Table {
        head: &["step", "action", "input", "state", "check"],
        rows: o
            .found
            .iter()
            .flatten()
            .enumerate()
            .map(|(k, l)| {
                vec![
                    (k + 1).to_string(),
                    l.step.clone(),
                    l.input.clone(),
                    l.state.clone(),
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
    [trace, tally]
        .into_iter()
        .filter(|t| !t.rows.is_empty())
        .collect()
}

fn summary(path: &str, d: &Diagram, run: Option<(&Config, &Outcome)>) -> String {
    let evidence = match d.verdict() {
        Some(Verdict::Fails { evidence, .. }) => evidence,
        _ => Vec::new(),
    };
    let head = format!(
        "{} {path} · {} · {}",
        match run.and_then(|(_, o)| o.found.as_ref()) {
            Some(_) => '✕',
            None => '✓',
        },
        d.check.as_ref().map_or(String::new(), |c| c.english()),
        evidence.join(" · ")
    );
    match run {
        None => head,
        Some((config, o)) => [head, format!("  {}", headline(config, o))]
            .into_iter()
            .chain(tables(o).iter().flat_map(Table::text))
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn section(path: &str, src: &str, d: &Diagram, theme: Theme) -> Result<String, String> {
    let (words, formula) = d
        .check
        .as_ref()
        .map_or((String::new(), String::new()), |c| {
            (c.english(), c.source())
        });
    Ok(format!(
        r#"<section><h2>{}</h2><code>{}</code><div class="d">{}</div><details><summary>{}</summary><pre>{}</pre></details></section>"#,
        escape(&words),
        escape(&formula),
        weft_draw::svg(d, theme)?,
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

fn checked(file: &Path, config: &Config, theme: Theme, drawn: bool) -> (String, String) {
    let path = file.strip_prefix(".").unwrap_or(file).display().to_string();
    diagram(file, config)
        .and_then(|(src, d)| match drawn {
            true => section(&path, &src, &d, theme).map(|s| (summary(&path, &d, None), s)),
            false => run(&d, file, config)
                .map(|run| (summary(&path, &d, Some((config, &run))), String::new())),
        })
        .unwrap_or_else(|e| (format!("✕ {path} · {e}"), String::new()))
}

fn report(drawn: bool) -> Result<Report, String> {
    let (files, config, theme) = files()?;
    let width = match drawn || config.transport.is_some() {
        true => 1,
        false => thread::available_parallelism().map_or(1, usize::from),
    };
    let next = AtomicUsize::new(0);
    let mut done: Vec<(usize, (String, String))> = thread::scope(|s| {
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
    let (lines, sections): (Vec<String>, Vec<String>) = done.into_iter().map(|(_, o)| o).unzip();
    Ok(Report {
        lines,
        sections: sections.concat(),
        theme,
    })
}

fn page(r: &Report) -> String {
    let wrong = r.lines.iter().filter(|l| l.starts_with('✕')).count();
    let errors: String = r
        .lines
        .iter()
        .filter(|l| l.starts_with('✕'))
        .map(|l| format!(r#"<p class="e">{}</p>"#, escape(l)))
        .collect();
    let p = r.theme.palette();
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Weft Report</title><style>
body{{margin:0;background:{bg};color:{fg};font-family:Inter,ui-sans-serif,system-ui,-apple-system,"Segoe UI",sans-serif;padding:24px max(16px,env(safe-area-inset-right)) 24px max(16px,env(safe-area-inset-left))}}
h1{{font-size:22px;margin:0 0 6px}}
h2{{font-size:17px;font-weight:600;margin:0 0 6px}}
code{{display:block;font:13px/1.5 ui-monospace,Menlo,monospace;color:{muted};margin:0 0 14px;overflow-wrap:anywhere}}
.e{{color:{critical}}}
p{{margin:0 0 8px;color:{muted}}}
main{{display:flex;flex-wrap:wrap;gap:0 48px}}
section{{flex:1 1 auto;max-width:100%;min-width:0;border-top:1px solid {surface};padding:20px 0}}
.d svg{{display:block;margin:0 auto}}
details{{margin-top:8px;color:{muted};font-size:12px}}
pre{{font:12px/1.5 ui-monospace,Menlo,monospace;background:{surface};color:{fg};padding:12px;border-radius:6px;overflow-x:auto}}
</style></head><body><h1>Weft Report</h1><p>{} · {wrong} wrong</p>{errors}<main>{}</main></body></html>"#,
        match r.lines.len() {
            1 => "1 file".to_string(),
            n => format!("{n} files"),
        },
        r.sections,
        bg = p.bg,
        fg = p.fg,
        muted = p.muted,
        surface = p.surface,
        critical = p.critical
    )
}

fn respond(mut stream: TcpStream) -> std::io::Result<()> {
    let mut request = String::new();
    BufReader::new(&stream).read_line(&mut request)?;
    let (status, body) = match request.split_whitespace().nth(1) {
        Some("/") => (
            "200 OK",
            report(true).map_or_else(|e| format!("<pre>{}</pre>", escape(&e)), |r| page(&r)),
        ),
        _ => ("404 Not Found", String::new()),
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\ncontent-type: text/html; charset=utf-8\r\ncache-control: no-store\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn serve() -> Result<(), String> {
    report(true)?;
    let listener = TcpListener::bind(ADDRESS).map_err(|e| format!("{ADDRESS}: {e}"))?;
    let url = format!("http://{ADDRESS}");
    println!("Weft Report: {url}");
    Command::new(OPEN)
        .arg(&url)
        .status()
        .map_err(|e| format!("{OPEN} {url}: {e}"))?;
    for stream in listener.incoming().flatten() {
        std::thread::spawn(|| respond(stream).unwrap_or_else(|e| eprintln!("{e}")));
    }
    Ok(())
}

fn check() -> Result<bool, String> {
    let r = report(false)?;
    r.lines.iter().for_each(|l| println!("{l}"));
    Ok(r.failed())
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
