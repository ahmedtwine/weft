use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use sourcemap::{DecodedMap, Token};

use crate::link::location::{self, Location};
use crate::lock;

const MAP: &str = "map";
const SCRIPT: &str = "js";
const RESCAN: Duration = Duration::from_secs(5);
const SKIPPED: [&str; 4] = ["node_modules", "target", ".git", ".turbo"];

struct Loaded {
    modified: SystemTime,
    map: Option<Arc<DecodedMap>>,
}

#[derive(Default)]
struct Served {
    names: HashMap<String, Vec<PathBuf>>,
    scanned: Option<Instant>,
}

pub(crate) struct Bundles {
    root: PathBuf,
    maps: Mutex<HashMap<PathBuf, Loaded>>,
    served: Mutex<Served>,
}

impl Bundles {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root,
            maps: Mutex::default(),
            served: Mutex::default(),
        }
    }

    pub(crate) fn original(&self, location: &str, line: u32, column: u32) -> Option<(String, u32)> {
        let file = self.bundle(location)?;
        let map = self.map(&file)?;
        let token = locate(&map, line.checked_sub(1)?, column.saturating_sub(1))?;
        Some((token.get_source()?.to_owned(), token.get_src_line() + 1))
    }

    fn bundle(&self, location: &str) -> Option<PathBuf> {
        match Location::new(location)? {
            Location::Served(path) => self.served(&path),
            Location::File(path) => Some(path).filter(|path| path.is_file()),
        }
    }

    fn map(&self, file: &Path) -> Option<Arc<DecodedMap>> {
        let modified = fs::metadata(file)
            .and_then(|metadata| metadata.modified())
            .ok()?;
        if let Some(loaded) = lock(&self.maps)
            .get(file)
            .filter(|loaded| loaded.modified == modified)
        {
            return loaded.map.clone();
        }
        let map = mapping(file).map(Arc::new);
        lock(&self.maps).insert(
            file.to_path_buf(),
            Loaded {
                modified,
                map: map.clone(),
            },
        );
        map
    }

    fn served(&self, path: &Path) -> Option<PathBuf> {
        let name = location::name(path)?;
        let mut served = lock(&self.served);
        if !served.names.contains_key(name)
            && served
                .scanned
                .is_none_or(|scanned| scanned.elapsed() >= RESCAN)
        {
            served.names = scan(&self.root);
            served.scanned = Some(Instant::now());
        }
        served
            .names
            .get(name)?
            .iter()
            .max_by_key(|candidate| {
                (
                    location::shared(candidate, path),
                    fs::metadata(candidate)
                        .and_then(|metadata| metadata.modified())
                        .ok(),
                )
            })
            .cloned()
    }
}

fn locate(map: &DecodedMap, line: u32, column: u32) -> Option<Token<'_>> {
    match map {
        DecodedMap::Index(index) => {
            let section = index
                .sections()
                .take_while(|section| section.get_offset() <= (line, column))
                .last()?;
            let (top, left) = section.get_offset();
            locate(
                section.get_sourcemap()?,
                line - top,
                if line == top { column - left } else { column },
            )
        }
        map => map
            .lookup_token(line, column)
            .filter(|token| token.get_dst_line() == line),
    }
}

fn mapping(file: &Path) -> Option<DecodedMap> {
    let sibling = file.with_added_extension(MAP);
    let path = if sibling.is_file() {
        sibling
    } else {
        let reference =
            sourcemap::locate_sourcemap_reference(fs::File::open(file).ok()?).ok()??;
        if let Some(map) = reference.get_embedded_sourcemap().ok()? {
            return Some(map);
        }
        reference.resolve_path(file)?
    };
    sourcemap::decode_slice(&fs::read(path).ok()?).ok()
}

fn scan(root: &Path) -> HashMap<String, Vec<PathBuf>> {
    let mut names: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let walker = ignore::WalkBuilder::new(root)
        .standard_filters(false)
        .filter_entry(|entry| {
            !entry
                .file_name()
                .to_str()
                .is_some_and(|name| SKIPPED.contains(&name))
        })
        .build();
    for entry in walker.filter_map(Result::ok) {
        let bundle = entry.path().with_extension("");
        if entry
            .path()
            .extension()
            .is_some_and(|extension| extension == MAP)
            && bundle
                .extension()
                .is_some_and(|extension| extension == SCRIPT)
            && let Some(name) = location::name(&bundle)
        {
            names
                .entry(name.to_owned())
                .or_default()
                .push(bundle.clone());
        }
    }
    names
}
