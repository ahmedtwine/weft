use std::path::Path;

use weft_parse::{Diagram, Ran, Span, State};
use weft_simulator::{Choice, Hello, Member, Report, Turn};

pub struct Setup<'a> {
    d: &'a Diagram,
    file: String,
    now: State,
    hello: Hello,
}

impl<'a> Setup<'a> {
    pub fn new(d: &'a Diagram, file: &Path) -> Setup<'a> {
        let none = || Choice::Object(Vec::new());
        Setup {
            d,
            file: file.display().to_string(),
            now: d.init.clone(),
            hello: Hello {
                reads: d
                    .init
                    .iter()
                    .flat_map(|(o, pairs)| pairs.keys().map(move |k| format!("{o}.{k}")))
                    .collect(),
                start: vec![none()],
                actions: d
                    .objects
                    .iter()
                    .flat_map(|o| &o.moves)
                    .map(|m| (m.verb.clone(), none()))
                    .collect(),
            },
        }
    }
}

impl Member for Setup<'_> {
    fn hello(&self) -> &Hello {
        &self.hello
    }

    fn turn(&mut self, turn: &Turn) -> Result<Report, String> {
        match turn {
            Turn::Start(_) => self.now = self.d.init.clone(),
            Turn::Run(verb, object, _) => {
                let to = self
                    .d
                    .enabled(&self.now)
                    .into_iter()
                    .find(|(o, v, _)| o.name == *object && v == verb)
                    .map(|(_, _, to)| to)
                    .ok_or_else(|| format!("`{object}` has no `on {verb}` line from its state"))?;
                self.now.insert(object.to_string(), to);
            }
            Turn::Read | Turn::Done => {}
        }
        Ok(Report::State(self.now.clone()))
    }

    fn finish(&mut self) -> Result<Ran, String> {
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
            start: self
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
