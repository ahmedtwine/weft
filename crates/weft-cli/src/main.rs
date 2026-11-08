use std::{env, fs, path::PathBuf, process::ExitCode};

use globset::{Glob, GlobSetBuilder};
use ignore::WalkBuilder;
use serde::Deserialize;
use weft_draw::Theme;
use weft_parse::{Diagram, Verdict};

const USAGE: &str = "usage: weft [--visualize] [--format ascii|html]";
const CONFIG: &str = "weft.hcl";
const SUFFIX: &str = ".weft.hcl";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    include: Vec<String>,
}

#[derive(PartialEq)]
enum Format {
    Ascii,
    Html,
}

fn files() -> Result<Vec<PathBuf>, String> {
    let source =
        fs::read_to_string(CONFIG).map_err(|_| format!("{CONFIG} is not in this directory"))?;
    let config: Config = hcl::from_str(&source).map_err(|e| format!("{CONFIG}: {e}"))?;
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
        false => Ok(found),
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn summary(path: &str, d: &Diagram) -> String {
    match d.verdict() {
        Some(Verdict::Fails { step, reason, .. }) => {
            format!(
                "✓ {path} · {reason} · the run breaks it at step {}",
                step + 1
            )
        }
        Some(Verdict::Holds) => format!("✕ {path} · the run breaks no check and no state machine"),
        None => format!("✓ {path} · no check"),
    }
}

fn section(path: &str, src: &str, d: &Diagram) -> Result<String, String> {
    Ok(format!(
        r#"<section><h2>{}</h2><div class="d light">{}</div><div class="d dark">{}</div><details><summary>{}</summary><pre>{}</pre></details></section>"#,
        escape(&d.title),
        weft_draw::svg(d, Theme::Light)?,
        weft_draw::svg(d, Theme::Dark)?,
        escape(path),
        escape(src)
    ))
}

fn page(sections: &str, lines: &[String]) -> String {
    let wrong = lines.iter().filter(|l| l.starts_with('✕')).count();
    let dark =
        "--bg:#161616;--fg:#e6e6e6;--muted:#9a9a9a;--line:#333333;--code:#1f1f1f;color-scheme:dark";
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Weft Report</title><style>
:root{{--bg:#ffffff;--fg:#1f1f1f;--muted:#6e6e6e;--line:#e6e6e6;--code:#f3f3f3;color-scheme:light}}
@media (prefers-color-scheme:dark){{:root:not([data-theme="light"]){{{dark}}}:root:not([data-theme="light"]) .light{{display:none}}:root:not([data-theme="light"]) .dark{{display:block}}}}
:root[data-theme="dark"]{{{dark}}}:root[data-theme="dark"] .light{{display:none}}:root[data-theme="dark"] .dark{{display:block}}
body{{margin:0;background:var(--bg);color:var(--fg);font-family:Inter,ui-sans-serif,system-ui,-apple-system,"Segoe UI",sans-serif;padding:24px max(16px,env(safe-area-inset-right)) 24px max(16px,env(safe-area-inset-left))}}
h1{{font-size:22px;margin:0 0 6px}}
h2{{font-size:16px;margin:0 0 12px}}
p{{margin:0 0 8px;color:var(--muted)}}
section{{border-top:1px solid var(--line);padding:20px 0}}
.d{{overflow-x:auto}}
.d svg{{display:block}}
.dark{{display:none}}
details{{margin-top:8px;color:var(--muted)}}
pre{{font:12px/1.5 ui-monospace,Menlo,monospace;background:var(--code);color:var(--fg);padding:12px;border-radius:6px;overflow-x:auto}}
</style></head><body><h1>Weft Report</h1><p>{} files · {wrong} wrong</p>{sections}</body></html>"#,
        lines.len()
    )
}

fn main() -> ExitCode {
    let mut visualize = false;
    let mut format = Format::Ascii;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match (arg.as_str(), &format) {
            ("--visualize", _) => visualize = true,
            ("--format", _) => match args.next().as_deref() {
                Some("ascii") => format = Format::Ascii,
                Some("html") => format = Format::Html,
                _ => {
                    eprintln!("{USAGE}");
                    return ExitCode::from(2);
                }
            },
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let files = match files() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let html = visualize && format == Format::Html;
    let mut failed = false;
    let mut lines = Vec::new();
    let mut sections = String::new();
    for file in files {
        let path = file
            .strip_prefix(".")
            .unwrap_or(&file)
            .display()
            .to_string();
        let parsed = fs::read_to_string(&file)
            .map_err(|e| e.to_string())
            .and_then(|src| {
                weft_parse::parse(&src)
                    .map(|d| (src, d))
                    .map_err(|e| e.to_string())
            });
        let (src, d) = match parsed {
            Ok(p) => p,
            Err(e) => {
                eprintln!("✕ {path} · {e}");
                failed = true;
                continue;
            }
        };
        let line = summary(&path, &d);
        failed |= line.starts_with('✕');
        let drawn = match (visualize, html) {
            (true, true) => section(&path, &src, &d).map(|s| {
                sections.push_str(&s);
                eprintln!("{line}");
            }),
            (true, false) => weft_draw::ascii(&d).map(|text| println!("{}\n\n{text}", d.title)),
            (false, _) => {
                println!("{line}");
                Ok(())
            }
        };
        if let Err(e) = drawn {
            eprintln!("✕ {path} · {e}");
            failed = true;
        }
        lines.push(line);
    }
    if html {
        print!("{}", page(&sections, &lines));
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
