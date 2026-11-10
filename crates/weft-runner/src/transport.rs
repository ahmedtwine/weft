use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use serde_json::{Value as Json, json};

pub const WAIT: Duration = Duration::from_secs(60);

pub trait Transport {
    fn schema(&mut self) -> Result<Json, String>;
    fn ask(&mut self, message: &Json) -> Result<Json, String>;
    fn done(&mut self) {}
}

pub struct Unix {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Unix {
    pub fn new(stream: UnixStream) -> Result<Unix, String> {
        stream.set_nonblocking(false).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(WAIT))
            .map_err(|e| e.to_string())?;
        let writer = stream.try_clone().map_err(|e| e.to_string())?;
        Ok(Unix {
            reader: BufReader::new(stream),
            writer,
        })
    }
}

impl Transport for Unix {
    fn schema(&mut self) -> Result<Json, String> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => Err("the runner closes the socket".to_string()),
            Ok(_) => serde_json::from_str(&line).map_err(|e| format!("{e}: {line}")),
            Err(e) => Err(e.to_string()),
        }
    }

    fn ask(&mut self, message: &Json) -> Result<Json, String> {
        writeln!(self.writer, "{message}").map_err(|e| e.to_string())?;
        self.schema()
    }

    fn done(&mut self) {
        let _ = writeln!(self.writer, "{}", json!({ "done": true }));
    }
}

pub struct Http {
    agent: ureq::Agent,
    url: String,
}

impl Transport for Http {
    fn schema(&mut self) -> Result<Json, String> {
        self.agent
            .get(self.url.as_str())
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("GET {}: {e}", self.url))
    }

    fn ask(&mut self, message: &Json) -> Result<Json, String> {
        self.agent
            .post(self.url.as_str())
            .send_json(message)
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("POST {}: {e}", self.url))
    }
}

pub fn connect(transport: &weft_parse::Transport) -> Result<Box<dyn Transport>, String> {
    Ok(match transport {
        weft_parse::Transport::Unix(path) => Box::new(Unix::new(
            UnixStream::connect(path).map_err(|e| format!("{path}: {e}"))?,
        )?),
        weft_parse::Transport::Http(url) => Box::new(Http {
            agent: ureq::Agent::new_with_defaults(),
            url: url.clone(),
        }),
    })
}
