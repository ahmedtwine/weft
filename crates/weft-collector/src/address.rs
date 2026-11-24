use std::net::Ipv6Addr;
use std::sync::LazyLock;

use percent_encoding::percent_decode_str;
use url::Url;

static BASE: LazyLock<Option<Url>> = LazyLock::new(|| Url::parse("http://route.invalid").ok());

fn resolve(raw: &str) -> Option<Url> {
    BASE.as_ref().and_then(|base| base.join(raw).ok())
}

pub(crate) fn path(raw: &str) -> Option<String> {
    resolve(raw).map(|url| url.path().to_owned())
}

pub(crate) fn segments(raw: &str) -> Vec<String> {
    resolve(raw)
        .iter()
        .filter_map(Url::path_segments)
        .flatten()
        .filter(|segment| !segment.is_empty())
        .map(|segment| percent_decode_str(segment).decode_utf8_lossy().into_owned())
        .collect()
}

pub(crate) fn endpoint(host: &str, port: Option<&str>) -> String {
    match (host.parse::<Ipv6Addr>(), port) {
        (Ok(_), Some(port)) => format!("[{host}]:{port}"),
        (_, Some(port)) => format!("{host}:{port}"),
        (_, None) => host.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{endpoint, path, segments};

    #[test]
    fn reads_targets_and_endpoints() {
        assert_eq!(path("/shop/a?x=1#top").as_deref(), Some("/shop/a"));
        assert_eq!(path("https://example.com/b/c?d").as_deref(), Some("/b/c"));
        assert_eq!(segments("https://example.com//a%20b/c/"), ["a b", "c"]);
        assert_eq!(endpoint("::1", Some("3000")), "[::1]:3000");
        assert_eq!(endpoint("example.com", Some("443")), "example.com:443");
        assert_eq!(endpoint("example.com", None), "example.com");
    }
}
