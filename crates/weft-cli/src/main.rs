use std::{env, fs, path::PathBuf, process::ExitCode};

use globset::{Glob, GlobSetBuilder};
use ignore::WalkBuilder;
use weft_draw::Theme;
use weft_parse::{Config, Diagram, Verdict};

const USAGE: &str = "usage: weft [--visualize] [--format ascii|html]";
const CONFIG: &str = "weft.kdl";
const SUFFIX: &str = ".weft.kdl";

#[derive(PartialEq)]
enum Format {
    Ascii,
    Html,
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

fn summary(path: &str, d: &Diagram) -> String {
    match d.verdict() {
        Some(Verdict::Fails {
            statement,
            step,
            evidence,
            ..
        }) => {
            let s = &d.steps[step];
            format!(
                "✓ {path} · {statement} · step {} {} from={} to={} · {}",
                step + 1,
                s.send.name,
                s.from,
                s.to,
                evidence.join(" · ")
            )
        }
        Some(Verdict::Holds) if d.checks.is_empty() => format!("✓ {path} · holds"),
        Some(Verdict::Holds) => format!("✕ {path} · the run breaks no invariant"),
        None => format!("✓ {path} · no check"),
    }
}

fn section(
    path: &str,
    src: &str,
    d: &Diagram,
    config: &Config,
    theme: Theme,
) -> Result<String, String> {
    let statements: String = d
        .checks
        .iter()
        .map(|c| format!(r#"<p class="s">{}</p>"#, escape(&c.statement)))
        .collect();
    Ok(format!(
        r#"<section><h2>{}</h2>{statements}<div class="d">{}</div><details><summary>source</summary><pre>{}</pre></details></section>"#,
        escape(path),
        weft_draw::svg(d, &config.styles, theme)?,
        escape(src)
    ))
}

fn page(sections: &str, lines: &[String], theme: Theme) -> String {
    let wrong = lines.iter().filter(|l| l.starts_with('✕')).count();
    let p = theme.palette();
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Weft Report</title><style>
body{{margin:0;background:{bg};color:{fg};font-family:Inter,ui-sans-serif,system-ui,-apple-system,"Segoe UI",sans-serif;padding:24px max(16px,env(safe-area-inset-right)) 24px max(16px,env(safe-area-inset-left))}}
h1{{font-size:22px;margin:0 0 6px}}
h2{{font-size:13px;margin:0 0 6px;color:{muted};font-weight:500}}
.s{{font-size:17px;font-weight:600;color:{fg};margin:0 0 14px}}
p{{margin:0 0 8px;color:{muted}}}
section{{border-top:1px solid {surface};padding:20px 0}}
.d{{overflow-x:auto}}
.d svg{{display:block}}
details{{margin-top:8px;color:{muted}}}
pre{{font:12px/1.5 ui-monospace,Menlo,monospace;background:{surface};color:{fg};padding:12px;border-radius:6px;overflow-x:auto}}
</style></head><body><h1>Weft Report</h1><p>{} files · {wrong} wrong</p>{sections}</body></html>"#,
        lines.len(),
        bg = p.bg,
        fg = p.fg,
        muted = p.muted,
        surface = p.surface
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
    let (files, config, theme) = match files() {
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
        let (src, d) = match parsed.and_then(|(src, d)| match d.unstyled(&config.styles) {
            Some(o) => Err(format!(
                "`{}` has no `style {}` in {CONFIG}",
                o.name, o.kind
            )),
            None => Ok((src, d)),
        }) {
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
            (true, true) => section(&path, &src, &d, &config, theme).map(|s| {
                sections.push_str(&s);
                eprintln!("{line}");
            }),
            (true, false) => weft_draw::ascii(&d).map(|text| println!("{path}\n\n{text}")),
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
        print!("{}", page(&sections, &lines, theme));
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
