use std::ffi::c_void;
use std::path::Path;

use weft_parse::{Diagram, Ran, Span, State};
use weft_simulator::{Hello, Mark, Member, Report, Tagged, Turn};

pub struct Setup<'a> {
    d: &'a Diagram,
    file: String,
    now: State,
    hello: Hello,
}

impl<'a> Setup<'a> {
    pub fn new(d: &'a Diagram, file: &Path) -> Setup<'a> {
        let tagged = |tag: String| Tagged {
            tag,
            ..Tagged::default()
        };
        Setup {
            d,
            file: file.strip_prefix(".").unwrap_or(file).display().to_string(),
            now: d.init.clone(),
            hello: Hello {
                reads: d
                    .init
                    .iter()
                    .flat_map(|(o, pairs)| pairs.keys().map(move |k| format!("{o}.{k}")))
                    .map(tagged)
                    .collect(),
                actions: d
                    .objects
                    .iter()
                    .flat_map(|o| &o.moves)
                    .map(|m| tagged(m.verb.clone()))
                    .collect(),
                mocks: Vec::new(),
            },
        }
    }
}

impl Member for Setup<'_> {
    fn hello(&self) -> &Hello {
        &self.hello
    }

    fn begin(&mut self, _: *mut c_void) {
        self.now = self.d.init.clone();
    }

    fn turn(&mut self, turn: &Turn) -> Result<Report, String> {
        let Turn::Run(verb, object) = turn else {
            return Ok(Report::State(self.now.clone()));
        };
        let to = self
            .d
            .enabled(&self.now)
            .into_iter()
            .find(|(o, v, _)| o.name == *object && v == verb)
            .map(|(_, _, to)| to)
            .ok_or_else(|| format!("`{object}` has no `on {verb}` line from its state"))?;
        self.now.insert(object.to_string(), to);
        Ok(Report::Ran(self.now.clone(), String::new()))
    }

    fn end(&mut self, _: &Turn) -> Result<Mark, String> {
        Ok(Mark {
            status: 0,
            origin: None,
        })
    }

    fn told(&mut self, _: &str) {}

    fn finish(&mut self, _: *mut c_void) -> Result<Ran, String> {
        let span = |line: usize| Span {
            file: self.file.clone(),
            from: line,
            to: line,
        };
        let mut actions: Vec<(String, Vec<Span>)> = Vec::new();
        for m in self.d.objects.iter().flat_map(|o| &o.moves) {
            match actions.iter_mut().find(|(verb, _)| *verb == m.verb) {
                Some((_, spans)) => spans.push(span(m.line)),
                None => actions.push((m.verb.clone(), vec![span(m.line)])),
            }
        }
        Ok(Ran {
            reset: self
                .d
                .objects
                .iter()
                .filter(|o| !o.initial.is_empty())
                .map(|o| span(o.line))
                .collect(),
            actions,
        })
    }
}
