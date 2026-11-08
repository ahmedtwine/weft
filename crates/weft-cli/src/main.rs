use std::{env, fs, path::PathBuf, process::ExitCode};

use weft_parse::{Diagram, Verdict};

const USAGE: &str = "usage: weft <file.weft|dir>... [--visualize] [--format ascii|html]";

#[derive(PartialEq)]
enum Format {
    Ascii,
    Html,
}

fn files(paths: Vec<PathBuf>) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for p in paths {
        if p.is_dir() {
            let mut found: Vec<PathBuf> = fs::read_dir(&p)?
                .map(|e| e.map(|e| e.path()))
                .collect::<Result<_, _>>()?;
            found.retain(|f| f.extension().is_some_and(|x| x == "weft"));
            found.sort();
            out.extend(found);
        } else {
            out.push(p);
        }
    }
    Ok(out)
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

fn page(sections: &str, lines: &[String]) -> String {
    let wrong = lines.iter().filter(|l| l.starts_with('✕')).count();
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Weft Report</title><style>
:root{{--bg:#ffffff;--fg:#1b2230;--muted:#5b6474;--line:#e3e7ee;--code:#f6f7f9;color-scheme:light}}
body{{margin:0;background:var(--bg);color:var(--fg);font-family:-apple-system,"Segoe UI",system-ui,sans-serif;padding:24px max(16px,env(safe-area-inset-right)) 24px max(16px,env(safe-area-inset-left))}}
h1{{font-size:22px;margin:0 0 6px}}
h2{{font-size:16px;margin:0 0 12px}}
p{{margin:0 0 8px;color:var(--muted)}}
section{{border-top:1px solid var(--line);padding:20px 0}}
.d{{overflow-x:auto}}
.d svg{{display:block}}
details{{margin-top:8px;color:var(--muted)}}
pre{{font:12px/1.5 ui-monospace,Menlo,monospace;background:var(--code);color:var(--fg);padding:12px;border-radius:6px;overflow-x:auto}}
</style></head><body><h1>Weft Report</h1><p>{} files · {wrong} wrong</p>{sections}</body></html>"#,
        lines.len()
    )
}

fn main() -> ExitCode {
    let mut visualize = false;
    let mut format = Format::Ascii;
    let mut paths = Vec::new();
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--visualize" => visualize = true,
            "--format" => match args.next().as_deref() {
                Some("ascii") => format = Format::Ascii,
                Some("html") => format = Format::Html,
                _ => {
                    eprintln!("{USAGE}");
                    return ExitCode::from(2);
                }
            },
            _ if arg.starts_with("--") => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
            _ => paths.push(PathBuf::from(arg)),
        }
    }
    let files = match files(paths) {
        Ok(f) if !f.is_empty() => f,
        Ok(_) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
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
        let path = file.display().to_string();
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
            (true, true) => weft_draw::svg(&d).map(|svg| {
                sections.push_str(&format!(
                    r#"<section><h2>{}</h2><div class="d">{svg}</div><details><summary>{}</summary><pre>{}</pre></details></section>"#,
                    escape(&d.title),
                    escape(&path),
                    escape(&src)
                ));
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
