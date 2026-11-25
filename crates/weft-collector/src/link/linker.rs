use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use crate::link::bundle::Bundles;
use crate::link::frame;
use crate::link::index::{Index, Map};
use crate::link::location;
use crate::link::route::Template;
use crate::link::{Evidence, Resolution, Site};
use crate::lock;

const MAPPER: &str = "spool";
const ROUTE: &str = "route";
const STACK: &str = "stack";
const CODE: &str = "code";
const CHAIN: usize = 8;

struct Entry {
    method: Option<String>,
    template: Template,
    handler: usize,
}

pub struct Linker {
    root: PathBuf,
    index: Index,
    routes: Vec<Entry>,
    bundles: Bundles,
    hints: HashMap<String, HashSet<String>>,
    claims: Mutex<HashMap<String, HashSet<String>>>,
}

impl Linker {
    pub fn new(source: &Path) -> Result<Self, String> {
        let root = source
            .canonicalize()
            .map_err(|e| format!("{}: {e}", source.display()))?;
        let mapped = Command::new(MAPPER)
            .arg("map")
            .arg(&root)
            .output()
            .map_err(|e| format!("{MAPPER}: {e}"))?;
        if !mapped.status.success() {
            return Err(String::from_utf8_lossy(&mapped.stderr).trim().to_string());
        }
        let map: Map =
            serde_json::from_slice(&mapped.stdout).map_err(|e| format!("{MAPPER}: {e}"))?;
        Ok(Self::mapped(root, map))
    }

    pub(crate) fn mapped(root: PathBuf, map: Map) -> Self {
        let (index, routes) = Index::new(map);
        let routes = routes
            .into_iter()
            .map(|route| Entry {
                template: Template::new(&route.path),
                method: route.method,
                handler: route.handler,
            })
            .collect();
        let mut hints: HashMap<String, HashSet<String>> = HashMap::new();
        for file in &index.files {
            let names = file
                .package
                .iter()
                .map(String::as_str)
                .chain(location::name(Path::new(&file.home)));
            for name in names.filter(|name| !name.is_empty()) {
                hints
                    .entry(normal(name))
                    .or_default()
                    .insert(file.home.clone());
            }
        }
        Self {
            bundles: Bundles::new(root.clone()),
            root,
            index,
            routes,
            hints,
            claims: Mutex::default(),
        }
    }

    fn framed(&self, evidence: &Evidence<'_>) -> Vec<(usize, u32)> {
        let mut places: Vec<(usize, u32)> = evidence
            .stack
            .into_iter()
            .flat_map(frame::frames)
            .filter_map(|frame| {
                self.index
                    .file(&self.root, frame.location)
                    .map(|file| (file, frame.line))
                    .or_else(|| {
                        let (source, line) =
                            self.bundles
                                .original(frame.location, frame.line, frame.column)?;
                        Some((self.index.file(&self.root, &source)?, line))
                    })
            })
            .take(CHAIN)
            .collect();
        places.dedup_by_key(|(file, line)| self.index.at(*file, *line));
        places
    }

    fn located(
        &self,
        evidence: &Evidence<'_>,
        framed: &[(usize, u32)],
    ) -> Option<(usize, u32, &'static str)> {
        framed
            .first()
            .map(|(file, line)| (*file, *line, STACK))
            .or_else(|| {
                let code = evidence.code?;
                Some((
                    self.index.file(&self.root, code.file)?,
                    code.line.unwrap_or(1),
                    CODE,
                ))
            })
    }

    fn served(&self, evidence: &Evidence<'_>) -> Option<&Entry> {
        let fitting = || {
            self.routes
                .iter()
                .filter(|entry| method_fits(entry.method.as_deref(), evidence.method))
        };
        let exact = evidence.route.map(Template::new).and_then(|route| {
            self.pick(
                evidence.service,
                fitting()
                    .filter(|entry| entry.template.key == route.key)
                    .collect(),
            )
        });
        exact.or_else(|| {
            let path = evidence.route.or(evidence.target.as_deref())?;
            self.pick(
                evidence.service,
                fitting()
                    .filter(|entry| entry.template.literals > 0 && entry.template.matches(path))
                    .collect(),
            )
        })
    }

    fn pick<'a>(&self, service: &str, candidates: Vec<&'a Entry>) -> Option<&'a Entry> {
        let (mine, others) = {
            let claims = lock(&self.claims);
            (
                claims.get(service).cloned(),
                claims
                    .iter()
                    .filter(|(owner, _)| owner.as_str() != service)
                    .flat_map(|(_, homes)| homes.iter().cloned())
                    .collect::<HashSet<String>>(),
            )
        };
        let owned = |entry: &&Entry| {
            mine.as_ref()
                .is_some_and(|homes| homes.contains(self.index.home(entry.handler)))
        };
        let allowed: Vec<&Entry> = candidates
            .into_iter()
            .filter(|entry| owned(entry) || !others.contains(self.index.home(entry.handler)))
            .collect();
        let preferred: Vec<&Entry> = match allowed.iter().any(owned) {
            true => allowed.into_iter().filter(owned).collect(),
            false => allowed,
        };
        let best = preferred
            .iter()
            .map(|entry| entry.template.literals)
            .max()?;
        let mut top = preferred
            .into_iter()
            .filter(|entry| entry.template.literals == best);
        let chosen = top.next()?;
        top.all(|other| other.handler == chosen.handler)
            .then_some(chosen)
    }

    fn seed(&self, service: &str) -> HashSet<String> {
        let name = normal(service);
        self.hints.get(&name).cloned().unwrap_or_else(|| {
            name.split('-')
                .filter_map(|token| self.hints.get(token))
                .flatten()
                .cloned()
                .collect()
        })
    }

    fn claim(&self, service: &str, home: Option<&str>) {
        lock(&self.claims)
            .entry(service.to_owned())
            .or_insert_with(|| self.seed(service))
            .extend(home.filter(|home| !home.is_empty()).map(str::to_owned));
    }

    pub(crate) fn resolve(&self, evidence: &Evidence<'_>) -> Resolution {
        let framed = self.framed(evidence);
        let located = self.located(evidence, &framed);
        let chain: Vec<Site> = framed
            .iter()
            .map(|(file, line)| self.index.site(self.index.at(*file, *line), Some(*line)))
            .collect();
        let home = located.map(|(file, _, _)| self.index.files[file].home.as_str());
        self.claim(evidence.service, home.filter(|_| !evidence.serves));
        let at = located.map(|(file, line, via)| Resolution {
            site: Some(self.index.site(self.index.at(file, line), Some(line))),
            via,
            chain: chain.clone(),
        });
        match evidence.serves {
            true => self
                .served(evidence)
                .map(|entry| Resolution {
                    site: Some(self.index.site(entry.handler, None)),
                    via: ROUTE,
                    chain,
                })
                .or_else(|| at.filter(|_| evidence.route.is_none()))
                .unwrap_or_default(),
            false => at.unwrap_or_default(),
        }
    }
}

fn method_fits(route: Option<&str>, request: Option<&str>) -> bool {
    match (route, request) {
        (Some(route), Some(request)) => {
            route.eq_ignore_ascii_case(request)
                || (route == "GET" && request.eq_ignore_ascii_case("HEAD"))
        }
        _ => true,
    }
}

fn normal(name: &str) -> String {
    name.to_ascii_lowercase().replace('_', "-")
}
