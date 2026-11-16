use std::ops::Range;

use serde_json::Value;

use crate::{Generator, Medium, Part, Rule};

pub struct Prose;

pub struct Code;

pub fn of(path: &str) -> Box<dyn Medium + Send + Sync> {
    match path.rsplit('.').next().unwrap_or_default() {
        "md" | "markdown" | "txt" => Box::new(Prose),
        _ => Box::new(Code),
    }
}

const STD: [&str; 11] = [
    "Vec", "String", "Option", "Result", "Ok", "Err", "Some", "None", "Self", "self", "format",
];

fn tidy(text: &str) -> String {
    let joined = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(" ,", ",")
        .replace(" .", ".")
        .replace(",.", ".")
        .replace(",,", ",");
    let trimmed = joined.trim_matches(|c: char| c == ',' || c.is_whitespace());
    let mut chars = trimmed.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn ended(text: String) -> String {
    if text.is_empty() || text.ends_with(['.', '!', '?', ':']) {
        text
    } else {
        text + "."
    }
}

fn clauses(text: &str) -> Vec<&str> {
    text.trim_end_matches(['.', '!', '?', ':'])
        .split(", ")
        .collect()
}

fn removed(text: &str, words: &str) -> Option<String> {
    let at = text.find(words)?;
    Some(format!("{} {}", &text[..at], &text[at + words.len()..]))
}

const WRAPPER: &str = "id: wrapper\nlanguage: rust\nrule:\n  any:\n    - pattern: fn $NAME($$$PARAMS) -> $RET { $BODY }\n    - pattern: pub fn $NAME($$$PARAMS) -> $RET { $BODY }";

const ECHO: &str = "id: echo\nlanguage: rust\nrule:\n  pattern: let $A = $A;";

fn names(rename: Option<(&str, &str)>) -> String {
    let rule = "id: name\nlanguage: rust\nrule:\n  any:\n    - kind: identifier\n    - kind: field_identifier\n    - kind: type_identifier";
    match rename {
        Some((from, to)) => format!("{rule}\n  regex: ^{from}$\nfix: {to}"),
        None => rule.into(),
    }
}

fn local(name: &str) -> String {
    format!(
        "id: local\nlanguage: rust\nrule:\n  kind: identifier\n  regex: ^{name}$\n  inside:\n    stopBy: end\n    any:\n      - kind: parameter\n      - kind: let_declaration"
    )
}

fn start(m: &Value) -> usize {
    m["range"]["byteOffset"]["start"]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or_default()
}

fn span(m: &Value) -> Range<usize> {
    start(m)
        ..m["range"]["byteOffset"]["end"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or_default()
}

fn apply(text: &str, mut matches: Vec<Value>) -> String {
    matches.sort_by_key(|m| std::cmp::Reverse(start(m)));
    matches.iter().fold(text.to_string(), |out, m| {
        let at = m["replacementOffsets"]["start"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or_else(|| start(m));
        let end = m["replacementOffsets"]["end"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or_else(|| span(m).end);
        format!(
            "{}{}{}",
            &out[..at],
            m["replacement"].as_str().unwrap_or_default(),
            &out[end..]
        )
    })
}

fn cut_out(text: &str, range: Range<usize>) -> String {
    let begin = text[..range.start].rfind('\n').map_or(0, |i| i + 1);
    let end = text[range.end..]
        .find('\n')
        .map_or(text.len(), |i| range.end + i + 1);
    let rest = format!("{}{}", &text[begin..range.start], &text[range.end..end]);
    let joined = if rest.trim().is_empty() {
        format!("{}{}", &text[..begin], &text[end..])
    } else {
        format!("{}{}", &text[..range.start], &text[range.end..])
    };
    joined.replace("\n\n\n", "\n\n")
}

fn cleaned(text: String) -> String {
    let echoes: Vec<Range<usize>> = grep(ECHO, &text).iter().map(span).collect();
    let consts = grep(&kinds(&["const_item".into()]), &text);
    let twins: Vec<Range<usize>> = consts
        .iter()
        .enumerate()
        .filter(|(i, m)| consts[..*i].iter().any(|n| n["text"] == m["text"]))
        .map(|(_, m)| span(m))
        .collect();
    let mut cuts: Vec<Range<usize>> = echoes.into_iter().chain(twins).collect();
    cuts.sort_by_key(|r| std::cmp::Reverse(r.start));
    cuts.into_iter().fold(text, |out, r| cut_out(&out, r))
}

fn valid(text: &str) -> bool {
    let dir = std::env::temp_dir().join(env!("CARGO_PKG_NAME"));
    let file = dir.join(format!("{}.rs", std::process::id()));
    std::fs::create_dir_all(&dir).is_ok()
        && std::fs::write(&file, text).is_ok()
        && std::process::Command::new("rustc")
            .args([
                "--edition",
                "2024",
                "--crate-type",
                "lib",
                "--emit=metadata",
                "-A",
                "warnings",
                "-o",
            ])
            .arg(dir.join(format!("{}.rmeta", std::process::id())))
            .arg(&file)
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
}

fn style(word: &str) -> u8 {
    match word.chars().next() {
        Some(c) if c.is_uppercase() && word.chars().all(|c| !c.is_lowercase()) => 2,
        Some(c) if c.is_uppercase() => 1,
        _ => 0,
    }
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn identifiers(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for m in grep(&names(None), text) {
        let word = m["text"].as_str().unwrap_or_default().to_string();
        if word.len() > 1 && !STD.contains(&word.as_str()) && !out.contains(&word) {
            out.push(word);
        }
    }
    out
}

fn swapped(text: &str, map: &[(String, String)]) -> String {
    let mut out = String::new();
    let mut word = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_alphanumeric() || c == '_' {
            word.push(c);
            continue;
        }
        out += map
            .iter()
            .find(|(from, _)| *from == word)
            .map_or(word.as_str(), |(_, to)| to.as_str());
        word.clear();
        out.push(c);
    }
    out.pop();
    out
}

impl Code {
    fn functions(text: &str) -> Vec<Range<usize>> {
        grep(&kinds(&["function_item".into()]), text)
            .iter()
            .map(span)
            .collect()
    }

    fn rename(artifact: &str, part: &Part, from: &str, to: &str) -> String {
        let functions = Self::functions(artifact);
        let declares = |r: &Range<usize>| !grep(&local(from), &artifact[r.clone()]).is_empty();
        let home = functions
            .iter()
            .filter(|r| r.contains(&part.span.start) && declares(r))
            .min_by_key(|r| r.len());
        let kept = |at: usize| match home {
            Some(r) => r.contains(&at),
            None => !functions.iter().any(|r| r.contains(&at) && declares(r)),
        };
        cleaned(apply(
            artifact,
            grep(&names(Some((from, to))), artifact)
                .into_iter()
                .filter(|m| kept(start(m)))
                .collect(),
        ))
    }

    fn home(artifact: &str, part: &Part) -> Range<usize> {
        Self::functions(artifact)
            .into_iter()
            .filter(|r| r.contains(&part.span.start))
            .min_by_key(Range::len)
            .unwrap_or(part.span.clone())
    }

    fn inline(artifact: &str, part: &Part, name: &str) -> Option<String> {
        let home = Self::home(artifact, part);
        let found = grep(WRAPPER, &artifact[home.clone()])
            .into_iter()
            .find(|m| m["metaVariables"]["single"]["NAME"]["text"] == name)?;
        let params: Vec<String> = found["metaVariables"]["multi"]["PARAMS"]
            .as_array()?
            .iter()
            .filter_map(|p| {
                p["text"]
                    .as_str()?
                    .split_once(':')
                    .map(|(n, _)| n.trim().to_string())
            })
            .collect();
        let map: Vec<(String, String)> = params
            .iter()
            .enumerate()
            .map(|(i, p)| (p.clone(), format!("$A{i}")))
            .collect();
        let body = swapped(
            found["metaVariables"]["single"]["BODY"]["text"].as_str()?,
            &map,
        );
        let call = format!(
            "{name}({})",
            map.iter()
                .map(|(_, a)| a.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let rule = format!("id: inline\nlanguage: rust\nrule:\n  pattern: {call}\nfix: {body:?}");
        let calls: Vec<Value> = grep(&rule, artifact)
            .into_iter()
            .filter(|m| !home.contains(&start(m)))
            .collect();
        let edited = apply(artifact, calls);
        let at = edited.find(&artifact[home.clone()])?;
        Some(cut_out(&edited, at..at + home.len()))
    }
}

fn grep(rules: &str, text: &str) -> Vec<Value> {
    let Ok(mut child) = std::process::Command::new("ast-grep")
        .args(["scan", "--inline-rules", rules, "--json=stream", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
    else {
        return Vec::new();
    };
    if let Some(mut stdin) = child.stdin.take() {
        std::io::Write::write_all(&mut stdin, text.as_bytes()).ok();
    }
    let out = child
        .wait_with_output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    out.lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn kinds(kinds: &[String]) -> String {
    kinds.iter().fold(
        "id: cut\nlanguage: rust\nrule:\n  any:".to_string(),
        |rule, kind| rule + &format!("\n    - kind: {kind}"),
    )
}

impl Medium for Prose {
    fn parts(&self, artifact: &str) -> Vec<Part> {
        let bytes = artifact.as_bytes();
        let mut out = Vec::new();
        let mut fenced = false;
        let mut at = 0;
        for line in artifact.split_inclusive('\n') {
            let start = at;
            at += line.len();
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                fenced ^= true;
            }
            if fenced || trimmed.is_empty() || trimmed.starts_with(['#', '|', '<', '-']) {
                continue;
            }
            let begin = start + line.len() - line.trim_start().len();
            let end = start + line.trim_end().len();
            let mut from = begin;
            for i in begin..end {
                let next = bytes.get(i + 2).copied().unwrap_or(b' ');
                if matches!(bytes[i], b'.' | b'!' | b'?')
                    && bytes.get(i + 1) == Some(&b' ')
                    && (next.is_ascii_uppercase() || next == b'`')
                {
                    out.push(Part {
                        span: from..i + 1,
                        text: artifact[from..i + 1].to_string(),
                    });
                    from = i + 2;
                }
            }
            if end > from {
                out.push(Part {
                    span: from..end,
                    text: artifact[from..end].to_string(),
                });
            }
        }
        out
    }

    fn rules(&self, _artifact: &str, part: &Part, marked: Range<usize>) -> Vec<Rule> {
        let pieces = clauses(&part.text);
        let first = part.text[marked].to_string();
        let removable = std::iter::once(first.clone())
            .chain(
                pieces
                    .iter()
                    .map(ToString::to_string)
                    .filter(|c| *c != first),
            )
            .map(Value::from)
            .collect();
        let last = i64::try_from(pieces.len()).unwrap_or(1) - 1;
        let mut out = vec![Rule {
            verb: "remove".into(),
            drawn: Generator::SampledFrom(removable),
        }];
        if last > 0 {
            out.push(Rule {
                verb: "keep".into(),
                drawn: Generator::Integer(0, last),
            });
            out.push(Rule {
                verb: "split".into(),
                drawn: Generator::SampledFrom(vec![", ".into()]),
            });
        }
        out
    }

    fn run(&self, artifact: &str, part: &Part, verb: &str, input: &str) -> Option<String> {
        let text = &part.text;
        let pieces = clauses(text);
        let variant = match verb {
            "remove" => ended(tidy(&removed(text, input)?)),
            "keep" => ended(tidy(pieces.get(input.parse::<usize>().ok()?)?)),
            "split" => text
                .trim_end_matches(['.', '!', '?', ':'])
                .split(input)
                .map(|c| ended(tidy(c)))
                .collect::<Vec<_>>()
                .join(" "),
            _ => return None,
        };
        (variant != *text && variant.len() > 1).then(|| {
            format!(
                "{}{variant}{}",
                &artifact[..part.span.start],
                &artifact[part.span.end..]
            )
        })
    }
}

impl Medium for Code {
    fn parts(&self, artifact: &str) -> Vec<Part> {
        let mut at = 0;
        artifact
            .split_inclusive('\n')
            .filter_map(|line| {
                let start = at + line.len() - line.trim_start().len();
                at += line.len();
                let text = line.trim();
                (!text.is_empty()).then(|| Part {
                    span: start..start + text.len(),
                    text: text.to_string(),
                })
            })
            .collect()
    }

    fn rules(&self, artifact: &str, part: &Part, marked: Range<usize>) -> Vec<Rule> {
        let name = &part.text[marked];
        let functions: Vec<String> = grep(&kinds(&["function_item".into()]), artifact)
            .iter()
            .filter_map(|m| {
                m["text"]
                    .as_str()
                    .and_then(|t| words(t).into_iter().skip_while(|w| w != "fn").nth(1))
            })
            .collect();
        let known: Vec<String> = identifiers(artifact)
            .into_iter()
            .filter(|w| !functions.contains(w))
            .collect();
        let plurals = known
            .iter()
            .filter(|w| style(w) < 2 && !w.ends_with('s'))
            .map(|w| format!("{w}s"))
            .filter(|w| !known.contains(w));
        let renames = known
            .iter()
            .cloned()
            .chain(plurals)
            .filter(|w| w != name && style(w) == style(name))
            .map(|w| Value::from(format!("{name} {w}")))
            .collect();
        let mut out = vec![
            Rule {
                verb: "rename".into(),
                drawn: Generator::SampledFrom(renames),
            },
            Rule {
                verb: "remove".into(),
                drawn: Generator::SampledFrom(
                    std::iter::once(Value::from(name))
                        .chain(
                            grep(&kinds(&["line_comment".into()]), &part.text)
                                .iter()
                                .filter_map(|m| m["text"].as_str().map(Value::from)),
                        )
                        .collect(),
                ),
            },
        ];
        if grep(WRAPPER, &artifact[Self::home(artifact, part)])
            .iter()
            .any(|m| m["metaVariables"]["single"]["NAME"]["text"] == name)
        {
            out.push(Rule {
                verb: "inline".into(),
                drawn: Generator::SampledFrom(vec![name.into()]),
            });
        }
        out
    }

    fn run(&self, artifact: &str, part: &Part, verb: &str, input: &str) -> Option<String> {
        let edited = match verb {
            "rename" => input
                .split_once(' ')
                .map(|(from, to)| Self::rename(artifact, part, from, to))?,
            "inline" => Self::inline(artifact, part, input)?,
            "remove" => part.text.find(input).map(|at| {
                cleaned(cut_out(
                    artifact,
                    part.span.start + at..part.span.start + at + input.len(),
                ))
            })?,
            _ => return None,
        };
        (edited != artifact && valid(&edited)).then_some(edited)
    }
}
