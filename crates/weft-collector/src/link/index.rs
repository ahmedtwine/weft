use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use serde::Deserialize;

use crate::link::Site;
use crate::link::location::{self, Location};

const VENDORED: &str = "node_modules";
const WALK_UP: usize = 2;

#[derive(Deserialize)]
pub(crate) struct File {
    pub(crate) path: String,
    pub(crate) home: String,
    pub(crate) package: Option<String>,
    root: usize,
}

#[derive(Deserialize)]
struct Symbol {
    file: usize,
    name: String,
    line: u32,
}

#[derive(Deserialize)]
pub(crate) struct Route {
    pub(crate) method: Option<String>,
    pub(crate) path: String,
    pub(crate) handler: usize,
}

#[derive(Deserialize)]
pub(crate) struct Map {
    files: Vec<File>,
    symbols: Vec<Symbol>,
    sites: Vec<(usize, u32, usize)>,
    declared: Vec<(usize, u32, usize)>,
    pub(crate) routes: Vec<Route>,
}

pub(crate) struct Index {
    pub(crate) files: Vec<File>,
    symbols: Vec<Symbol>,
    paths: HashMap<String, usize>,
    names: HashMap<String, Vec<usize>>,
    owners: Vec<HashMap<u32, usize>>,
    declared: Vec<Vec<(u32, usize)>>,
}

impl Index {
    pub(crate) fn new(map: Map) -> (Self, Vec<Route>) {
        let mut names: HashMap<String, Vec<usize>> = HashMap::new();
        for (index, file) in map.files.iter().enumerate() {
            names
                .entry(basename(&file.path).to_owned())
                .or_default()
                .push(index);
        }
        let mut owners = vec![HashMap::new(); map.files.len()];
        for (file, line, symbol) in map.sites {
            owners[file].insert(line, symbol);
        }
        let mut declared = vec![Vec::new(); map.files.len()];
        for (file, line, symbol) in map.declared {
            declared[file].push((line, symbol));
        }
        declared.iter_mut().for_each(|lines| lines.sort_unstable());
        let index = Self {
            paths: map
                .files
                .iter()
                .enumerate()
                .map(|(index, file)| (file.path.clone(), index))
                .collect(),
            files: map.files,
            symbols: map.symbols,
            names,
            owners,
            declared,
        };
        (index, map.routes)
    }

    pub(crate) fn file(&self, root: &Path, raw: &str) -> Option<usize> {
        let Location::File(path) = Location::new(raw)? else {
            return None;
        };
        if path.iter().any(|component| component == VENDORED) {
            return None;
        }
        location::relative(&path, root)
            .ancestors()
            .take(WALK_UP + 1)
            .find_map(|path| {
                path.to_str()
                    .and_then(|key| self.paths.get(key).copied())
                    .or_else(|| self.suffix(path))
            })
    }

    fn suffix(&self, path: &Path) -> Option<usize> {
        let mut best: Vec<(usize, usize)> = self
            .names
            .get(location::name(path)?)?
            .iter()
            .map(|file| {
                (
                    location::shared(Path::new(&self.files[*file].path), path),
                    *file,
                )
            })
            .collect();
        best.sort_unstable_by_key(|(score, _)| Reverse(*score));
        let (score, file) = *best.first()?;
        (score >= 2 && best.get(1).is_none_or(|next| next.0 < score)).then_some(file)
    }

    pub(crate) fn at(&self, file: usize, line: u32) -> usize {
        self.owners[file]
            .get(&line)
            .copied()
            .or_else(|| {
                self.declared[file]
                    .iter()
                    .rev()
                    .find(|(declared, _)| *declared <= line)
                    .map(|(_, symbol)| *symbol)
            })
            .unwrap_or(self.files[file].root)
    }

    pub(crate) fn home(&self, symbol: usize) -> &str {
        &self.files[self.symbols[symbol].file].home
    }

    pub(crate) fn site(&self, symbol: usize, line: Option<u32>) -> Site {
        let at = &self.symbols[symbol];
        Site {
            symbol: Arc::from(at.name.as_str()),
            source: Arc::from(format!(
                "{}:{}",
                self.files[at.file].path,
                line.unwrap_or(at.line)
            )),
        }
    }
}

fn basename(path: &str) -> &str {
    location::name(Path::new(path)).unwrap_or(path)
}
