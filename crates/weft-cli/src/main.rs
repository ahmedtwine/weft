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
use weft_parse::{Config, Diagram};
use weft_simulator::{Outcome, view};

const USAGE: &str = "usage: weft [--ui | <file>…]";
const CONFIG: &str = "weft.kdl";
const SUFFIX: &str = ".weft.kdl";
const ADDRESS: &str = "127.0.0.1:7357";
const INPUT: usize = 80;
const DRAWN: &str = "* draw ";
const SHARE: &str = " of test cases";
static PAGE: Mutex<Option<(Option<SystemTime>, String)>> = Mutex::new(None);

struct Report {
    lines: Vec<String>,
    sections: String,
    checks: u64,
}

struct Checked {
    line: String,
    section: String,
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
    let trace = Table {
        head: &["step", "action", "input", "state", "check"],
        rows: o
            .trace
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
    let drawn = Table {
        head: &["draw", "cases"],
        rows: o
            .output
            .iter()
            .filter_map(|line| line.trim().strip_prefix(DRAWN)?.rsplit_once(": "))
            .map(|(value, share)| {
                vec![value.to_string(), share.trim_end_matches(SHARE).to_string()]
            })
            .collect(),
    };
    [trace, tally, code, drawn]
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

fn section(path: &str, src: &str, d: &Diagram, o: &Outcome) -> Result<String, String> {
    let rules: String = invariants(d)
        .iter()
        .map(|rule| format!("<li><code>{}</code></li>", escape(rule)))
        .collect();
    Ok(format!(
        r#"<section><h2>{}</h2><ul class="unstyled">{rules}</ul><div class="tabs" role="tablist"><button type="button" role="tab" aria-selected="true">Diagram</button><button type="button" role="tab" aria-selected="false">Run</button></div><div class="d" role="tabpanel">{}</div><div role="tabpanel" hidden>{}<details><summary>{}</summary><pre>{}</pre></details></div></section>"#,
        escape(path),
        weft_draw::svg(d, None, &trace::code(d, o))?,
        trace::panel(d, o)?,
        escape(path),
        escape(src)
    ))
}

fn run(d: &Diagram, file: &Path, config: &Config) -> Result<Outcome, String> {
    let key = fs::canonicalize(file).map_err(|e| e.to_string())?;
    weft_runner::run(config, d, file, key.to_str())
}

fn diagram(file: &Path, config: &Config) -> Result<(String, Diagram), String> {
    let src = fs::read_to_string(file).map_err(|e| e.to_string())?;
    let d = weft_parse::parse(&src, config).map_err(|e| e.to_string())?;
    Ok((src, d))
}

fn checked(file: &Path, config: &Config, drawn: bool) -> Checked {
    let path = file.strip_prefix(".").unwrap_or(file).display().to_string();
    let started = Instant::now();
    diagram(file, config)
        .and_then(|(src, d)| {
            let o = run(&d, file, config)?;
            Ok(Checked {
                line: summary(&path, &d, config, &o, started.elapsed()),
                section: match drawn {
                    true => section(&path, &src, &d, &o)?,
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

fn report(drawn: bool, only: &[String]) -> Result<Report, String> {
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
                        files.get(i).map(|file| (i, checked(file, &config, drawn)))
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
    let icon = |name| weft_draw::icon(name).unwrap_or_default();
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Weft Report</title><script>try{{const t=localStorage.getItem("weft-theme");if(t==="light"||t==="dark")document.documentElement.dataset.theme=t}}catch{{}}</script><link rel="preconnect" href="https://fonts.googleapis.com"><link rel="preconnect" href="https://fonts.gstatic.com" crossorigin><link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500;600&display=swap"><link rel="stylesheet" href="https://unpkg.com/@knadh/oat@1.1.0/oat.min.css"><script src="https://unpkg.com/@knadh/oat@1.1.0/oat.min.js" defer></script><script src="https://unpkg.com/pretty-json-custom-element@1.1.19/index.js" defer></script><style>
:root{{color-scheme:light dark;
--pele-bg:light-dark(#ffffff,#161616);--pele-fg:light-dark(#1f1f1f,#e6e6e6);--pele-muted:light-dark(#6e6e6e,#9a9a9a);--pele-line:light-dark(#1f1f1f,#e6e6e6);
--pele-surface:light-dark(#f3f3f3,#222222);--pele-surface-alt:light-dark(#e6e6e6,#2c2c2c);--pele-border:light-dark(#8a8a8a,#4a4a4a);
--pele-accent:light-dark(#5b7bd5,#7b98e6);--pele-critical:light-dark(#e45756,#f07574);
--pele-font:Inter,ui-sans-serif,system-ui,-apple-system,"Segoe UI",sans-serif;--pele-font-mono:"JetBrains Mono",ui-monospace,SFMono-Regular,Menlo,monospace;
--rule:light-dark(#e6e6e6,#2c2c2c);
--background:var(--pele-bg);--foreground:var(--pele-fg);--card:var(--pele-bg);--card-foreground:var(--pele-fg);--muted:var(--pele-surface);--muted-foreground:var(--pele-muted);
--faint:var(--pele-surface);--faint-foreground:var(--pele-muted);--border:var(--rule);--primary:var(--pele-accent);--danger:var(--pele-critical);--success:light-dark(#54a24b,#6cc062);
--font-sans:var(--pele-font);--font-mono:var(--pele-font-mono)}}
:root[data-theme=light]{{color-scheme:light}}
:root[data-theme=dark]{{color-scheme:dark}}
body{{margin:0;padding:var(--space-8) var(--space-6);background:var(--pele-bg);color:var(--pele-fg);font-family:var(--pele-font);-webkit-font-smoothing:antialiased}}
main{{max-width:1440px;margin:0 auto}}
header{{display:flex;align-items:center;justify-content:space-between;gap:var(--space-4);margin-bottom:var(--space-6)}}
h1{{font-size:var(--text-3);font-weight:700;letter-spacing:-.02em;margin:0}}
header p{{margin:var(--space-1) 0 0;color:var(--pele-muted);font-size:var(--text-7)}}
#theme{{display:grid;place-items:center;width:36px;height:36px;padding:0;border:1px solid var(--rule);border-radius:999px;background:var(--pele-surface);color:var(--pele-fg);cursor:pointer}}
#theme svg{{width:18px;height:18px;fill:none;stroke:currentColor;stroke-width:1.75;stroke-linecap:round;stroke-linejoin:round}}
#theme svg{{display:none}}
:root:not([data-theme]) #theme .auto,:root[data-theme=light] #theme .sun,:root[data-theme=dark] #theme .moon{{display:block}}
[role=alert]{{font-family:var(--pele-font-mono);font-size:var(--text-7);border-radius:8px}}
h2{{font-size:var(--text-6);font-weight:600;font-family:var(--pele-font-mono);margin:0 0 var(--space-2)}}
section{{border:1px solid var(--rule);border-radius:14px;padding:var(--space-6);margin:var(--space-5) 0;background:var(--pele-bg)}}
ul.unstyled code{{font-family:var(--pele-font-mono);font-size:12px;background:var(--pele-surface);border-radius:6px;padding:2px 6px}}
.d{{overflow-x:auto}}
.d svg{{display:block;margin:var(--space-4) auto}}
ul.unstyled{{margin:0}}
.tabs{{display:flex;gap:var(--space-4);margin:var(--space-4) 0 0;border-bottom:1px solid var(--rule)}}
.tabs button{{all:unset;cursor:pointer;padding:var(--space-2) 0;margin-bottom:-1px;border-bottom:2px solid transparent;color:var(--pele-muted);font-size:var(--text-7);font-weight:500}}
.tabs button[aria-selected=true]{{color:var(--pele-fg);border-bottom-color:var(--pele-fg)}}
.t{{max-height:26rem;overflow:auto;margin:var(--space-4) 0;border:1px solid var(--rule);border-radius:10px}}
.t table{{font-size:var(--text-7);margin:0}}
.t th{{position:sticky;top:0;background:var(--pele-surface);white-space:nowrap;font-family:var(--pele-font-mono);font-weight:500}}
.t td{{vertical-align:top;max-width:32ch;overflow-wrap:anywhere;font-family:var(--pele-font-mono)}}
.t td:first-child{{white-space:nowrap}}
.t td svg{{color:var(--success);vertical-align:-2px}}
.t tr.e td{{color:var(--danger)}}
.t tr.e td svg{{color:var(--danger)}}
mark{{background:color-mix(in srgb,var(--pele-accent) 18%,transparent);color:inherit;border-radius:4px;padding:0 3px}}
details summary{{color:var(--pele-muted);font-family:var(--pele-font-mono);font-size:var(--text-7)}}
details pre{{font:12px/18px var(--pele-font-mono);background:var(--pele-surface);border:0;border-radius:10px}}
pretty-json{{display:block;--font-family:var(--pele-font-mono);--font-size:12px;--indent:1rem;--key-color:var(--foreground);--string-color:var(--success);--number-color:var(--primary);--boolean-color:var(--primary);--null-color:var(--muted-foreground);--brace-color:var(--muted-foreground);--bracket-color:var(--muted-foreground);--comma-color:var(--muted-foreground);--arrow-color:var(--muted-foreground);--ellipsis-color:var(--muted-foreground)}}
foreignObject>div{{height:100%}}
figure{{display:flex;flex-direction:column;box-sizing:border-box;height:100%;margin:0;border:1px solid var(--rule);border-radius:10px;overflow:hidden;background:var(--pele-bg);box-shadow:0 1px 2px rgb(0 0 0/.04)}}
figcaption{{padding:4px 10px;border-bottom:1px solid var(--rule);background:var(--pele-surface);color:var(--pele-muted);font:10px/14px var(--pele-font-mono);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}}
figure pre{{flex:1;margin:0;padding:5px 0;border:0;border-radius:0;background-color:var(--pele-bg)!important;font:11px/15px var(--pele-font-mono);overflow:hidden}}
.shiki span{{color:light-dark(var(--shiki-light),var(--shiki-dark))}}
pre code{{counter-reset:n calc(var(--from) - 1);background:none;padding:0}}
pre .line::before{{counter-increment:n;content:counter(n);display:inline-block;min-width:4ch;margin-right:2ch;text-align:right;color:var(--pele-muted);opacity:.6}}
</style></head><body><main><header><div><h1>Weft Report</h1><p>{} · {wrong} wrong</p></div><button id="theme" type="button" aria-label="Theme: system, light or dark" title="Theme: system, light or dark"><svg class="auto" viewBox="0 0 24 24">{}</svg><svg class="sun" viewBox="0 0 24 24">{}</svg><svg class="moon" viewBox="0 0 24 24">{}</svg></button></header>{errors}{}</main><script>
document.addEventListener("click", (event) => {{
  const tab = event.target.closest("[role=tab]");
  if (!tab) return;
  const tabs = [...tab.parentElement.children];
  const panels = [...tab.closest("section").querySelectorAll(":scope > [role=tabpanel]")];
  tabs.forEach((t, i) => {{
    t.setAttribute("aria-selected", t === tab);
    panels[i].hidden = t !== tab;
  }});
}});
document.getElementById("theme").addEventListener("click", () => {{
  const root = document.documentElement;
  const next = {{ light: "dark", dark: "" }}[root.dataset.theme ?? ""] ?? "light";
  try {{
    if (next) localStorage.setItem("weft-theme", next);
    else localStorage.removeItem("weft-theme");
  }} catch {{}}
  if (next) root.dataset.theme = next;
  else delete root.dataset.theme;
}});
</script><script type="module">
import {{ codeToHtml, bundledLanguages }} from "https://esm.sh/shiki@4.6.0";
for (const pre of document.querySelectorAll("pre[data-lang]")) {{
  const lang = pre.dataset.lang in bundledLanguages ? pre.dataset.lang : "text";
  const box = document.createElement("div");
  box.innerHTML = await codeToHtml(pre.textContent, {{ lang, themes: {{ light: "github-light", dark: "github-dark" }}, defaultColor: false }});
  box.firstElementChild.style.setProperty("--from", pre.dataset.from);
  pre.replaceWith(box.firstElementChild);
}}
</script></body></html>"#,
        match r.lines.len() {
            1 => "1 file".to_string(),
            n => format!("{n} files"),
        },
        icon("sun-moon"),
        icon("sun"),
        icon("moon"),
        r.sections
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
            let page = report(true, &[])
                .map_or_else(|e| format!("<pre>{}</pre>", escape(&e)), |r| page(&r));
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
    files(&[])?;
    let listener = TcpListener::bind(ADDRESS).map_err(|e| format!("{ADDRESS}: {e}"))?;
    println!("Weft Report: http://{ADDRESS}");
    thread::spawn(current);
    for stream in listener.incoming().flatten() {
        std::thread::spawn(|| respond(stream).unwrap_or_else(|e| eprintln!("{e}")));
    }
    Ok(())
}

fn check(only: &[String]) -> Result<bool, String> {
    let started = Instant::now();
    let r = report(false, only)?;
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
        [ui] if ui == "--ui" => serve().map(|_| false),
        only if only.iter().all(|named| !named.starts_with('-')) => check(only),
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
