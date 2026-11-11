use std::net::{Ipv4Addr, Ipv6Addr};

use serde_json::{Value as Json, json};
use weft_hegel::{Bound, Case, Engine, Generator, Halt, VALID};

const CASES: u64 = 50;

fn each(mut body: impl FnMut(&Case) -> Result<(), Halt>) -> Vec<String> {
    let mut engine = Engine::start(CASES, None).expect("engine");
    while let Some(case) = engine.case().expect("case") {
        match body(&case) {
            Ok(()) | Err(Halt::Overrun | Halt::Invalid) => {}
            Err(Halt::Empty) => panic!("a pool has no value"),
            Err(Halt::Retry) => panic!("a recursion ends with a retry"),
            Err(Halt::Fatal(e)) => panic!("{e}"),
        }
        case.complete(VALID, None).expect("complete");
    }
    let ended = engine.end().expect("end");
    assert!(!ended.failed);
    ended.output
}

fn drawn(generator: &Generator, mut fits: impl FnMut(&Json) -> bool) {
    each(|case| {
        let value = case.draw(generator)?;
        assert!(fits(&value), "{value}");
        Ok(())
    });
}

fn text(value: &Json) -> &str {
    value.as_str().expect("a text")
}

fn leaves(tree: &Json) -> usize {
    tree.as_array()
        .map_or(1, |all| all.iter().map(leaves).sum())
}

fn depth(tree: &Json) -> usize {
    tree.as_array()
        .map_or(0, |all| 1 + all.iter().map(depth).max().unwrap_or(0))
}

#[test]
fn each_single_value_generator_gives_a_value_of_its_kind() {
    drawn(&Generator::Boolean, Json::is_boolean);
    drawn(&Generator::Integer(3, 7), |v| {
        (3..=7).contains(&v.as_i64().unwrap_or(0))
    });
    drawn(&Generator::Float(Bound::At(0.0), Bound::Past(1.0)), |v| {
        (0.0..1.0).contains(&v.as_f64().unwrap_or(-1.0))
    });
    drawn(&Generator::Bytes(2, Some(4)), |v| {
        [4, 8].contains(&text(v).len())
    });
    drawn(&Generator::Text(1, Some(3)), |v| {
        (1..=3).contains(&text(v).chars().count())
    });
    drawn(&Generator::Regex("^[a-c]{2}$".into()), |v| {
        text(v).len() == 2 && text(v).chars().all(|c| ('a'..='c').contains(&c))
    });
    drawn(&Generator::Email, |v| text(v).contains('@'));
    drawn(&Generator::Url, |v| text(v).starts_with("http"));
    drawn(&Generator::Domain, |v| text(v).contains('.'));
    drawn(&Generator::Uuid, |v| text(v).len() == 36);
    drawn(&Generator::Ipv4, |v| text(v).parse::<Ipv4Addr>().is_ok());
    drawn(&Generator::Ipv6, |v| text(v).parse::<Ipv6Addr>().is_ok());
    drawn(&Generator::Date, |v| text(v).len() == 10);
    drawn(&Generator::Time, |v| text(v).len() == 8);
    drawn(&Generator::Datetime, |v| {
        text(v).len() == 20 && text(v).ends_with('Z')
    });
    drawn(
        &Generator::Grammar("start: \"a\" | \"b\" start".into(), None),
        |v| text(v).ends_with('a') && text(v).trim_end_matches('a').chars().all(|c| c == 'b'),
    );
    drawn(&Generator::Constant(json!("one")), |v| v == "one");
    drawn(&Generator::SampledFrom(vec![json!(1), json!("two")]), |v| {
        v == 1 || v == "two"
    });
}

#[test]
fn each_combinator_gives_a_value_of_its_parts() {
    let digit = || Generator::Integer(0, 9);
    drawn(&Generator::OneOf(vec![digit(), Generator::Boolean]), |v| {
        v.is_boolean() || v.is_i64()
    });
    drawn(&Generator::Optional(Box::new(digit())), |v| {
        v.is_null() || v.is_i64()
    });
    drawn(&Generator::List(Box::new(digit()), 2, Some(5), true), |v| {
        let all = v.as_array().expect("a list");
        (2..=5).contains(&all.len())
            && all
                .iter()
                .all(|a| all.iter().filter(|b| a == *b).count() == 1)
    });
    drawn(
        &Generator::Dict(
            Box::new(Generator::SampledFrom(vec![json!("a"), json!("b")])),
            Box::new(digit()),
        ),
        |v| {
            v.as_object()
                .expect("a dict")
                .keys()
                .all(|k| k == "a" || k == "b")
        },
    );
    drawn(&Generator::Tuple(vec![digit(), Generator::Boolean]), |v| {
        v[0].is_i64() && v[1].is_boolean()
    });
    let mut absent = 0;
    drawn(
        &Generator::Record(vec![
            ("id".into(), digit()),
            (
                "name".into(),
                Generator::Optional(Box::new(Generator::Boolean)),
            ),
        ]),
        |v| {
            absent += usize::from(v.get("name").is_none());
            v["id"].is_i64() && v.get("name").is_none_or(Json::is_boolean)
        },
    );
    assert!(absent > 0, "each record has the optional field");
}

#[test]
fn a_recursive_generator_obeys_the_limits_of_hegel() {
    let tree = |depth, leaves| {
        Generator::Recursive(
            "tree".into(),
            Box::new(Generator::OneOf(vec![
                Generator::Integer(0, 9),
                Generator::List(Box::new(Generator::Subtree("tree".into())), 1, None, false),
            ])),
            depth,
            leaves,
        )
    };
    let mut most = (0, 0);
    drawn(&tree(Some(3), Some(8)), |v| {
        most = (most.0.max(depth(v)), most.1.max(leaves(v)));
        depth(v) <= 3 && leaves(v) <= 8
    });
    assert!(
        most.0 > 1 && most.1 > 2,
        "the trees have {most:?} levels and leaves"
    );
    drawn(&tree(None, None), |v| leaves(v) <= 100);
}

#[test]
fn a_pool_gives_a_value_that_an_earlier_draw_added() {
    let id = Generator::Add("ids".into(), Box::new(Generator::Integer(1, 1000)));
    let ids = Generator::List(Box::new(id), 1, Some(5), false);
    let reused = Generator::Pool("ids".into(), false);
    let consumed = Generator::Pool("ids".into(), true);
    each(|case| {
        case.pool("ids")?;
        assert!(matches!(case.draw(&reused), Err(Halt::Empty)));
        let added = case.draw(&ids)?;
        let added = added.as_array().expect("a list");
        assert_eq!(case.size("ids"), added.len());
        assert!(added.contains(&case.draw(&reused)?));
        assert_eq!(case.size("ids"), added.len());
        assert!(added.contains(&case.draw(&consumed)?));
        assert_eq!(case.size("ids"), added.len() - 1);
        case.undo()?;
        assert_eq!(case.size("ids"), added.len());
        Ok(())
    });
}

#[test]
fn an_event_generator_gives_each_value_to_the_statistics_of_hegel() {
    let sampled = Generator::Event(
        "draw tagName".into(),
        Box::new(Generator::SampledFrom(vec![json!("div"), json!("span")])),
    );
    let output = each(|case| case.draw(&sampled).map(drop));
    for value in ["div", "span"] {
        let label = format!("draw tagName \"{value}\"");
        assert!(
            output.iter().any(|line| line.contains(&label)),
            "{output:?}"
        );
    }
}
