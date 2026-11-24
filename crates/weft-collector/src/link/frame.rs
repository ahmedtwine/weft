use winnow::ModalResult;
use winnow::ascii::{dec_uint, space0};
use winnow::combinator::{alt, delimited, eof, opt, peek, preceded, repeat_till, terminated};
use winnow::prelude::*;
use winnow::token::{any, take_till, take_until};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Frame<'a> {
    pub(crate) location: &'a str,
    pub(crate) line: u32,
    pub(crate) column: u32,
}

pub(crate) fn frames(stack: &str) -> impl Iterator<Item = Frame<'_>> {
    stack.lines().filter_map(|line| frame.parse(line).ok())
}

fn frame<'a>(input: &mut &'a str) -> ModalResult<Frame<'a>> {
    delimited(space0, alt((v8, gecko)), (space0, eof)).parse_next(input)
}

fn v8<'a>(input: &mut &'a str) -> ModalResult<Frame<'a>> {
    preceded(
        "at ",
        alt((
            preceded((take_until(0.., " ("), " ("), terminated(place, ')')),
            place,
        )),
    )
    .parse_next(input)
}

fn gecko<'a>(input: &mut &'a str) -> ModalResult<Frame<'a>> {
    preceded(opt((take_till(0.., ['@', '/']), '@')), place).parse_next(input)
}

fn place<'a>(input: &mut &'a str) -> ModalResult<Frame<'a>> {
    (
        repeat_till::<_, _, (), _, _, _, _>(1.., any, peek(position)).take(),
        position,
    )
        .map(|(location, (line, column))| Frame {
            location,
            line,
            column,
        })
        .parse_next(input)
}

fn position(input: &mut &str) -> ModalResult<(u32, u32)> {
    terminated(
        (preceded(':', dec_uint), preceded(':', dec_uint)),
        peek(alt((")".void(), (space0, eof).void()))),
    )
    .parse_next(input)
}

#[cfg(test)]
mod tests {
    use super::{Frame, frames};

    #[test]
    fn reads_v8_and_gecko_frames() {
        let stack = "    at handler (/srv/build/chunks/route.js:3766:21)\n    at async Promise.all (index 0)\n    at file:///srv/main.js:4:9\nload@https://example.com/static/a.js:12:5\nhttps://example.com/vendor/@scope/b.js:1:2";
        assert_eq!(
            frames(stack).collect::<Vec<_>>(),
            [
                Frame {
                    location: "/srv/build/chunks/route.js",
                    line: 3766,
                    column: 21
                },
                Frame {
                    location: "file:///srv/main.js",
                    line: 4,
                    column: 9
                },
                Frame {
                    location: "https://example.com/static/a.js",
                    line: 12,
                    column: 5
                },
                Frame {
                    location: "https://example.com/vendor/@scope/b.js",
                    line: 1,
                    column: 2
                },
            ]
        );
    }
}
