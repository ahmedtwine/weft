use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Error, ErrorKind, Read, Result, Write as _};
use std::path::{Path, PathBuf};

const LIMIT: u64 = 256 * 1024 * 1024;

pub(crate) struct Spool {
    path: PathBuf,
    file: File,
    size: u64,
}

impl Spool {
    pub(crate) fn open(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let size = file.metadata()?.len();
        Ok(Self { path, file, size })
    }

    pub(crate) fn records(path: &Path) -> Result<Vec<(u8, u8, Vec<u8>)>> {
        let mut records = Vec::new();
        for file in [rotated(path), path.to_path_buf()] {
            match File::open(&file) {
                Ok(file) => read(BufReader::new(file), &mut records)?,
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(records)
    }

    pub(crate) fn append(&mut self, signal: u8, format: u8, body: &[u8]) -> Result<()> {
        if self.size >= LIMIT {
            fs::rename(&self.path, rotated(&self.path))?;
            self.file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            self.size = 0;
        }
        let length = u32::try_from(body.len()).map_err(Error::other)?;
        let mut record = Vec::with_capacity(body.len() + 6);
        record.extend_from_slice(&[signal, format]);
        record.extend_from_slice(&length.to_le_bytes());
        record.extend_from_slice(body);
        self.file.write_all(&record)?;
        self.size += record.len() as u64;
        Ok(())
    }
}

fn rotated(path: &Path) -> PathBuf {
    path.with_extension("old")
}

fn read(mut reader: impl Read, records: &mut Vec<(u8, u8, Vec<u8>)>) -> Result<()> {
    let mut header = [0_u8; 6];
    loop {
        match reader.read_exact(&mut header) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error),
        }
        let [signal, format, a, b, c, d] = header;
        let mut body = vec![0_u8; u32::from_le_bytes([a, b, c, d]) as usize];
        match reader.read_exact(&mut body) {
            Ok(()) => records.push((signal, format, body)),
            Err(error) if error.kind() == ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error),
        }
    }
}
