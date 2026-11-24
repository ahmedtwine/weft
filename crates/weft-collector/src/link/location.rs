use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use percent_encoding::percent_decode_str;
use url::Url;

pub(crate) enum Location {
    Served(PathBuf),
    File(PathBuf),
}

impl Location {
    pub(crate) fn new(raw: &str) -> Option<Self> {
        match Url::parse(raw) {
            Ok(url) if matches!(url.scheme(), "http" | "https") => {
                Some(Self::Served(decoded(url.path())))
            }
            Ok(url) if url.scheme() == "file" => url.to_file_path().ok().map(Self::File),
            Ok(url) if url.cannot_be_a_base() => None,
            Ok(url) => Some(Self::File(decoded(url.path()))),
            Err(_) => Some(Self::File(decoded(raw))),
        }
    }
}

pub(crate) fn relative(path: &Path, root: &Path) -> PathBuf {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .filter(|component| matches!(component, Component::Normal(_)))
        .collect()
}

pub(crate) fn shared(left: &Path, right: &Path) -> usize {
    left.components()
        .rev()
        .zip(right.components().rev())
        .take_while(|(left, right)| left == right)
        .count()
}

pub(crate) fn name(path: &Path) -> Option<&str> {
    path.file_name().and_then(OsStr::to_str)
}

fn decoded(text: &str) -> PathBuf {
    PathBuf::from(percent_decode_str(text).decode_utf8_lossy().as_ref())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Location, relative, shared};

    #[test]
    fn reads_each_location_form() {
        let path = |raw: &str| {
            Location::new(raw).map(|(Location::Served(path) | Location::File(path))| path)
        };
        assert_eq!(
            path("file:///srv/x/%5Bid%5D/route.ts").as_deref(),
            Some(Path::new("/srv/x/[id]/route.ts"))
        );
        assert_eq!(
            path("https://example.com/static/a.js?v=1").as_deref(),
            Some(Path::new("/static/a.js"))
        );
        assert_eq!(
            path("bundler:///[project]/src/page.tsx").as_deref(),
            Some(Path::new("/[project]/src/page.tsx"))
        );
        assert_eq!(
            path("/srv/plain.ts").as_deref(),
            Some(Path::new("/srv/plain.ts"))
        );
        assert_eq!(path("data:text/plain,x"), None);
        assert_eq!(
            relative(Path::new("/repo/./src/a.ts"), Path::new("/repo")),
            Path::new("src/a.ts")
        );
        assert_eq!(
            shared(Path::new("/x/src/a/b.ts"), Path::new("src/a/b.ts")),
            3
        );
    }
}
