use winnow::ModalResult;
use winnow::combinator::{alt, delimited, eof, opt, preceded, repeat, terminated};
use winnow::prelude::*;
use winnow::token::{one_of, rest, take_till, take_while};

use crate::address;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Part {
    Literal(String),
    Param,
    Rest { optional: bool },
}

#[derive(Clone, Debug)]
pub(crate) struct Template {
    parts: Vec<Part>,
    pub(crate) key: String,
    pub(crate) literals: usize,
}

impl Template {
    pub(crate) fn new(raw: &str) -> Self {
        let parts: Vec<Part> = template
            .parse(raw)
            .unwrap_or_default()
            .into_iter()
            .flatten()
            .collect();
        let key = parts.iter().fold(String::new(), |mut key, part| {
            key.push('/');
            key.push_str(match part {
                Part::Literal(text) => text,
                Part::Param => ":",
                Part::Rest { .. } => "*",
            });
            key
        });
        let literals = parts
            .iter()
            .filter(|part| matches!(part, Part::Literal(_)))
            .count();
        Self {
            parts,
            key: if key.is_empty() {
                String::from("/")
            } else {
                key
            },
            literals,
        }
    }

    pub(crate) fn matches(&self, target: &str) -> bool {
        let segments = address::segments(target);
        let mut at = 0;
        for part in &self.parts {
            match part {
                Part::Rest { optional } => return segments.len() > at || *optional,
                Part::Param if at < segments.len() => at += 1,
                Part::Literal(text) if segments.get(at) == Some(text) => at += 1,
                _ => return false,
            }
        }
        at == segments.len()
    }
}

fn template(input: &mut &str) -> ModalResult<Vec<Option<Part>>> {
    terminated(
        repeat(0.., preceded(take_while(0.., '/'), segment)),
        take_while(0.., '/'),
    )
    .parse_next(input)
}

fn segment(input: &mut &str) -> ModalResult<Option<Part>> {
    take_till(1.., '/')
        .and_then(terminated(
            alt((
                hidden.value(None),
                bracket.map(Some),
                brace.map(Some),
                angle.map(Some),
                colon.map(Some),
                star.map(Some),
                literal.map(Some),
            )),
            eof,
        ))
        .parse_next(input)
}

fn name<'a>(input: &mut &'a str) -> ModalResult<&'a str> {
    take_while(0.., |character: char| {
        character.is_alphanumeric() || character == '_' || character == '-'
    })
    .parse_next(input)
}

fn hidden(input: &mut &str) -> ModalResult<()> {
    alt((
        delimited('(', take_till(0.., ')'), ')').void(),
        preceded('@', rest).void(),
    ))
    .parse_next(input)
}

fn bracket(input: &mut &str) -> ModalResult<Part> {
    alt((
        delimited("[[...", name, "]]").value(Part::Rest { optional: true }),
        delimited("[...", name, ']').value(Part::Rest { optional: false }),
        delimited('[', name, ']').value(Part::Param),
    ))
    .parse_next(input)
}

fn brace(input: &mut &str) -> ModalResult<Part> {
    delimited(
        '{',
        (
            opt('*'),
            name,
            opt(alt((
                "...".value(true),
                preceded(':', name).map(|kind| kind == "path"),
            ))),
        ),
        '}',
    )
    .map(|(star, _, spread)| {
        if star.is_some() || spread == Some(true) {
            Part::Rest { optional: false }
        } else {
            Part::Param
        }
    })
    .parse_next(input)
}

fn angle(input: &mut &str) -> ModalResult<Part> {
    delimited('<', (opt(terminated(name, ':')), name), '>')
        .map(|(kind, _)| {
            if kind == Some("path") {
                Part::Rest { optional: false }
            } else {
                Part::Param
            }
        })
        .parse_next(input)
}

fn colon(input: &mut &str) -> ModalResult<Part> {
    preceded(':', (name, opt(one_of(['*', '+', '?']))))
        .map(|(_, modifier)| match modifier {
            Some('*') => Part::Rest { optional: true },
            Some('+') => Part::Rest { optional: false },
            _ => Part::Param,
        })
        .parse_next(input)
}

fn star(input: &mut &str) -> ModalResult<Part> {
    preceded('*', name)
        .value(Part::Rest { optional: false })
        .parse_next(input)
}

fn literal(input: &mut &str) -> ModalResult<Part> {
    rest.map(|text: &str| Part::Literal(text.to_owned()))
        .parse_next(input)
}

#[cfg(test)]
mod tests {
    use super::Template;

    #[test]
    #[expect(
        clippy::literal_string_with_formatting_args,
        reason = "route templates use braces"
    )]
    fn keys_cover_each_router_syntax() {
        for (raw, key) in [
            ("/orders/[orderId]", "/orders/:"),
            ("/(group)/carts/@modal/[[...rest]]", "/carts/*"),
            ("/docs/[...slug]", "/docs/*"),
            ("/users/{id}/files/{*path}", "/users/:/files/*"),
            ("/items/{id:int}/{rest:path}", "/items/:/*"),
            ("/flask/<int:id>/<path:file>", "/flask/:/*"),
            ("/express/:id/:rest*", "/express/:/*"),
            ("/static/*", "/static/*"),
            ("", "/"),
            ("///health//", "/health"),
        ] {
            assert_eq!(Template::new(raw).key, key, "{raw}");
        }
    }

    #[test]
    fn matches_targets_by_segment() {
        let template = Template::new("/shops/[shop]/[item]");
        assert!(template.matches("/shops/north.example.com/7?view=full"));
        assert!(template.matches("https://example.com/shops/a%20b/c#top"));
        assert!(!template.matches("/shops/one"));
        assert!(Template::new("/docs/[[...slug]]").matches("/docs"));
        assert!(!Template::new("/docs/[...slug]").matches("/docs"));
    }
}
