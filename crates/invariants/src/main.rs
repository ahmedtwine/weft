use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use invariants::jev::Jev;
use invariants::narrow::{Narrowing, Step};
use invariants::{Choice, Measure, Medium, Part, Result, Rule, flagged, medium, sameness, scores};
use kdl::{KdlDocument, KdlNode};

const USAGE: &str = "usage: invariants narrow <dir> | golden <dir> | check <file> [<dir>]";

struct Golden {
    name: String,
    measure: Measure,
    fix: Option<String>,
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn named(dir: &Path, stem: &str) -> Result<PathBuf> {
    std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.file_stem().is_some_and(|s| s == stem))
        .ok_or(format!("no {stem}.* in {}", dir.display()))
}

fn document(path: &Path) -> Result<KdlDocument> {
    read(path)?
        .parse()
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn strings(node: &KdlNode) -> Vec<String> {
    node.entries()
        .iter()
        .filter(|e| e.name().is_none())
        .filter_map(|e| e.value().as_string().map(str::to_string))
        .collect()
}

fn props(doc: &KdlDocument, name: &str) -> BTreeMap<String, String> {
    doc.get(name)
        .map(|n| {
            n.entries()
                .iter()
                .filter_map(|e| {
                    Some((
                        e.name()?.value().to_string(),
                        e.value().as_string()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn choice(doc: &KdlDocument) -> Choice {
    Choice {
        cut: doc.get("cut").map(strings).unwrap_or_default(),
        question: doc
            .get_arg("question")
            .and_then(|v| v.as_string())
            .unwrap_or_default()
            .to_string(),
        flag: props(doc, "flag"),
        pass: props(doc, "pass"),
        split: props(doc, "split"),
        then: doc
            .get("then")
            .and_then(KdlNode::children)
            .map(|d| Box::new(choice(d))),
    }
}

fn nodes(path: &Path, name: &str, key: &str) -> Result<Vec<(String, String)>> {
    Ok(document(path)?
        .nodes()
        .iter()
        .filter(|n| n.name().value() == name)
        .filter_map(|n| {
            let first = n
                .entries()
                .iter()
                .find(|e| e.name().is_none())?
                .value()
                .as_string()?
                .to_string();
            let value = n
                .entries()
                .iter()
                .find(|e| e.name().is_some_and(|k| k.value() == key))?
                .value()
                .as_string()?
                .to_string();
            Some((first, value))
        })
        .collect())
}

fn golden(dir: &Path) -> Result<Vec<Golden>> {
    Ok(document(&dir.join("golden.kdl"))?
        .nodes()
        .iter()
        .filter(|n| n.name().value() == "golden")
        .filter_map(|n| {
            let body = n.children()?;
            Some(Golden {
                name: strings(n).into_iter().next()?,
                measure: Measure::Choice(choice(body)),
                fix: body
                    .get_arg("fix")
                    .and_then(|v| v.as_string())
                    .map(str::to_string),
            })
        })
        .collect())
}

fn shown(measure: &Measure) -> String {
    match measure {
        Measure::Pattern(words) => format!("pattern {words:?}"),
        Measure::Choice(Choice { question, .. }) => format!("{question:?}"),
    }
}

fn flags(jev: &Jev, rules: &[(String, Rule)], file: &Path) -> Result<()> {
    let text = read(file)?;
    let medium = medium::of(&file.to_string_lossy());
    let parts = medium.parts(&text);
    let texts: Vec<String> = parts.iter().map(|p| p.text.clone()).collect();
    for (name, rule) in rules {
        let scores = scores(jev, medium.as_ref(), &rule.measure, &texts, &text)?;
        for (part, score) in parts
            .iter()
            .zip(scores)
            .filter(|(_, s)| *s >= rule.threshold)
        {
            let line = text[..part.span.start].matches('\n').count() + 1;
            println!(
                "{}:{line}  {name}  {score:.2}  {}",
                file.display(),
                part.text
            );
        }
    }
    Ok(())
}

fn words(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut from = None;
    for (i, c) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        match (from, c.is_alphanumeric() || c == '_') {
            (None, true) => from = Some(i),
            (Some(at), false) => {
                out.push((at, &text[at..i]));
                from = None;
            }
            _ => {}
        }
    }
    out
}

fn declared(text: &str, part: &Part) -> Vec<std::ops::Range<usize>> {
    let all = words(text);
    words(&part.text)
        .into_iter()
        .filter(|(at, word)| {
            all.iter()
                .find(|(_, w)| w == word)
                .is_some_and(|(first, _)| *first == part.span.start + at)
        })
        .map(|(at, word)| at..at + word.len())
        .collect()
}

fn fixed(
    jev: &Jev,
    medium: &dyn Medium,
    text: &str,
    part: &Part,
    gold: &Golden,
    verb: &str,
) -> Result<Option<(String, usize)>> {
    let mut inputs: Vec<String> = declared(text, part)
        .into_iter()
        .flat_map(|m| medium.actions(text, part, m))
        .filter(|a| a.verb == verb)
        .flat_map(|a| a.drawn.values())
        .collect();
    inputs.sort();
    inputs.dedup();
    let before = flagged(jev, medium, &gold.measure, text)?.len();
    let mut best: Option<(String, usize, f64)> = None;
    for (input, edited) in inputs.into_iter().filter_map(|input| {
        medium
            .run(text, part, verb, &input)
            .map(|edited| (input, edited))
    }) {
        let left = flagged(jev, medium, &gold.measure, &edited)?;
        let after = left.len();
        if after >= before || left.contains(&part.span.start) {
            continue;
        }
        let same = match input.split_once(' ') {
            Some((from, to)) if verb == "rename" => sameness(jev, text, from, to)?,
            _ => 1.0,
        };
        if best
            .as_ref()
            .is_none_or(|(_, left, kept)| (-same, after) < (-kept, *left))
        {
            best = Some((input, after, same));
        }
    }
    Ok(best.map(|(input, left, _)| (input, left)))
}

fn graded(dir: &Path) -> Result<()> {
    let jev = Jev::open()?;
    let test = named(dir, "test")?;
    let text = read(&test)?;
    let medium = medium::of(&test.to_string_lossy());
    let parts = medium.parts(&text);
    let texts: Vec<String> = parts.iter().map(|p| p.text.clone()).collect();
    let expected = nodes(&dir.join("expected.kdl"), "flag", "rule")?;
    let mut perfect = true;
    for gold in golden(dir)? {
        let scores = scores(&jev, medium.as_ref(), &gold.measure, &texts, &text)?;
        let marked: Vec<usize> = expected
            .iter()
            .filter(|(_, rule)| *rule == gold.name)
            .filter_map(|(words, _)| {
                (0..texts.len())
                    .filter(|&i| texts[i].contains(words.as_str()))
                    .min_by_key(|&i| texts[i].len())
            })
            .collect();
        let wanted = |i: usize| marked.contains(&i);
        let missed: Vec<&String> = (0..texts.len())
            .filter(|&i| wanted(i) && scores[i] < 0.5)
            .map(|i| &texts[i])
            .collect();
        let wrong: Vec<&String> = (0..texts.len())
            .filter(|&i| !wanted(i) && scores[i] >= 0.5)
            .map(|i| &texts[i])
            .collect();
        let total = marked.len();
        perfect &= missed.is_empty() && wrong.is_empty();
        println!(
            "{:10} found {}/{total}, false flags {}  {}",
            gold.name,
            total - missed.len(),
            wrong.len(),
            shown(&gold.measure)
        );
        missed.iter().for_each(|t| println!("    missed  {t}"));
        wrong.iter().for_each(|t| println!("    false   {t}"));
        for part in (0..parts.len())
            .filter(|&i| wanted(i) && scores[i] >= 0.5)
            .map(|i| &parts[i])
        {
            let Some(verb) = &gold.fix else { continue };
            let line = part.text.lines().next().unwrap_or_default();
            match fixed(&jev, medium.as_ref(), &text, part, &gold, verb)? {
                Some((input, left)) => {
                    println!("    fix     {verb} {input:?} leaves {left} flags  {line}")
                }
                None => {
                    perfect = false;
                    println!("    unfixed {line}");
                }
            }
        }
    }
    if perfect {
        Ok(())
    } else {
        Err("a golden rule misses a part or flags a wrong part".into())
    }
}

fn narrow(dir: &Path) -> Result<()> {
    let jev = Jev::open()?;
    let artifact = named(dir, "artifact")?;
    let text = read(&artifact)?;
    let medium = medium::of(&artifact.to_string_lossy());
    let parts = medium.parts(&text);
    let marks = nodes(&dir.join("annotations.kdl"), "annotate", "note")?;
    let gold: Vec<Measure> = golden(dir)?.into_iter().map(|g| g.measure).collect();
    let mut input = std::io::stdin().lock().lines();
    for (n, (marked, note)) in marks.iter().enumerate() {
        let at = text
            .find(marked.as_str())
            .ok_or(format!("{marked:?} is not in {}", artifact.display()))?;
        let index = parts
            .iter()
            .enumerate()
            .filter(|(_, p)| p.span.contains(&at))
            .min_by_key(|(_, p)| p.span.len())
            .map(|(i, _)| i)
            .ok_or(format!("{marked:?} is in no part"))?;
        let offset = at - parts[index].span.start;
        let mut narrowing = Narrowing::new(
            medium.as_ref(),
            &text,
            &parts,
            index,
            offset..offset + marked.len(),
            gold.clone(),
        );
        println!(
            "\n{} · annotation {} of {} · {note:?}\n  part      {}",
            dir.display(),
            n + 1,
            marks.len(),
            parts[index].text
        );
        loop {
            let step = narrowing.step(&jev, medium.as_ref(), &text)?;
            println!(
                "  evidence  {} rejected, {} accepted",
                narrowing.rejected.len(),
                narrowing.accepted.len()
            );
            let held: Vec<String> = narrowing
                .checks
                .iter()
                .filter(|c| c.held)
                .map(|c| shown(&c.measure))
                .collect();
            if !narrowing.accepted.is_empty() {
                println!(
                    "  holds     {} of {} measures",
                    held.len(),
                    narrowing.checks.len()
                );
                held.iter().for_each(|m| println!("            {m}"));
            }
            match step {
                Step::Show(probe) => {
                    println!("  narrow    {}\n            {}", probe.from, probe.text);
                    print!("  reject (r), accept (a), next annotation (n)? ");
                    std::io::Write::flush(&mut std::io::stdout()).ok();
                    match input
                        .next()
                        .transpose()
                        .map_err(|e| e.to_string())?
                        .as_deref()
                        .map(str::trim)
                    {
                        Some("r") => narrowing.react(false),
                        Some("a") => narrowing.react(true),
                        Some("n") => break,
                        Some(_) => continue,
                        None => return Ok(()),
                    }
                }
                Step::Found(rule) => {
                    println!(
                        "  rule      {} at {:.2}, {}",
                        shown(&rule.measure),
                        rule.threshold,
                        if gold.contains(&rule.measure) {
                            "golden"
                        } else {
                            "outside the golden set"
                        }
                    );
                    let mut rules = invariants::load();
                    rules.push(rule.clone());
                    invariants::save(&rules)?;
                    flags(&jev, &[("found".into(), rule)], &named(dir, "test")?)?;
                    break;
                }
                Step::Failed => {
                    println!("  failed    each measure breaks a condition");
                    break;
                }
            }
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let done = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["narrow", dir] => narrow(Path::new(dir)),
        ["golden", dir] => graded(Path::new(dir)),
        ["check", file] => Jev::open().and_then(|jev| {
            flags(
                &jev,
                &invariants::load()
                    .into_iter()
                    .map(|r| ("saved".into(), r))
                    .collect::<Vec<_>>(),
                Path::new(file),
            )
        }),
        ["check", file, dir] => golden(Path::new(dir)).and_then(|gold| {
            Jev::open().and_then(|jev| {
                flags(
                    &jev,
                    &gold
                        .into_iter()
                        .map(|g| {
                            (
                                g.name,
                                Rule {
                                    measure: g.measure,
                                    threshold: 0.5,
                                    rejected: Vec::new(),
                                    accepted: Vec::new(),
                                },
                            )
                        })
                        .collect::<Vec<_>>(),
                    Path::new(file),
                )
            })
        }),
        _ => Err(USAGE.into()),
    };
    match done {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
