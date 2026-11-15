use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use invariants::artifact::{Artifact, example};
use invariants::{Result, THRESHOLD, scores};

fn flow(file: &Path) -> Result<()> {
    let artifact = Artifact::open(file)?;
    let example = example(&file.with_extension("json"))?;
    println!("golden invariants");
    let golden = artifact.wrong(&example, &example.invariants)?;
    let expected: BTreeMap<&String, BTreeSet<usize>> = example
        .violations
        .iter()
        .map(|(name, texts)| (name, artifact.expected(texts)))
        .collect();
    let owner = |marked: &str| {
        expected
            .iter()
            .find(|(_, parts)| {
                parts
                    .iter()
                    .any(|&p| artifact.parts[p].text.contains(marked))
            })
            .map(|(name, _)| name.to_string())
    };
    let machines = artifact.narrow(&example, |marked, alternate| {
        let name = owner(marked)?;
        let measure = example.invariants.get(&name)?.clone();
        let scored = scores(
            &artifact.jev,
            &BTreeMap::from([(name.clone(), measure)]),
            &[alternate.text.as_str()],
        )
        .ok()?;
        Some(scored.first()?.get(&name)? >= &THRESHOLD)
    })?;
    println!("flow");
    let mut failed = 0;
    for ((marked, _), machine) in example.annotations.iter().zip(&machines) {
        let Some(name) = owner(marked) else {
            println!("  golden  {marked:?} is in no golden violation");
            failed += 1;
            continue;
        };
        let target = &expected[&name];
        let flagged = artifact.flagged(&machine.invariants.iter().cloned().collect())?;
        let none = BTreeSet::new();
        let parts = |candidate: &str| flagged.get(candidate).unwrap_or(&none);
        let wrong = |candidate: &str| parts(candidate).symmetric_difference(target).count();
        let Some((best, measure)) = machine.invariants.iter().min_by_key(|(c, _)| wrong(c)) else {
            continue;
        };
        let chosen = machine.invariant().map(|i| i.name);
        match chosen.as_deref() {
            _ if wrong(best) > 0 => {
                println!(
                    "  draw  {marked:?}  no drawn invariant gives {name}; the nearest is {best}: {measure}, {} wrong",
                    wrong(best)
                );
                artifact.differ(parts(best), target);
                failed += 1;
            }
            Some(chosen) if wrong(chosen) == 0 => {
                println!("  pass  {marked:?}  {chosen} gives {name}")
            }
            chosen => {
                println!(
                    "  reactions  {marked:?}  the reactions chose {}, but {best} gives {name}",
                    chosen.unwrap_or("no invariant")
                );
                if let Some(chosen) = chosen {
                    artifact.differ(parts(chosen), target);
                }
                failed += 1;
            }
        }
    }
    match (golden, failed) {
        (0, 0) => Ok(()),
        _ => Err(format!(
            "{}: {golden} wrong violations with the golden invariants, {failed} of {} annotations fail in the flow",
            file.display(),
            example.annotations.len()
        )),
    }
}

fn path(name: &str) -> PathBuf {
    Path::new("examples").join(name)
}

#[test]
fn writing() {
    flow(&path("writing.md")).unwrap();
}

#[test]
fn code() {
    flow(&path("code.rs")).unwrap();
}
