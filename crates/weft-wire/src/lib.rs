use std::io::{self, Read, Write};

pub const JOIN: u8 = 0;
pub const TURN: u8 = 1;
pub const DONE: u8 = 2;
const MOST: usize = 1 << 28;

#[derive(Debug)]
pub struct Short;

pub struct Writer(pub Vec<u8>);

impl Writer {
    pub fn call(call: u8, part: &[u8]) -> Writer {
        let mut w = Writer(vec![call]);
        w.bytes(part);
        w
    }

    pub fn reply(code: i32, part: &[u8]) -> Writer {
        let mut w = Writer(code.to_le_bytes().to_vec());
        w.bytes(part);
        w
    }

    fn bytes(&mut self, value: &[u8]) {
        self.0
            .extend_from_slice(&(value.len() as u64).to_le_bytes());
        self.0.extend_from_slice(value);
    }
}

pub struct Reader<'a>(pub &'a [u8]);

impl<'a> Reader<'a> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], Short> {
        let (taken, rest) = self.0.split_first_chunk::<N>().ok_or(Short)?;
        self.0 = rest;
        Ok(*taken)
    }

    pub fn call(&mut self) -> Result<u8, Short> {
        self.take::<1>().map(|[call]| call)
    }

    pub fn code(&mut self) -> Result<i32, Short> {
        self.take().map(i32::from_le_bytes)
    }

    pub fn bytes(&mut self) -> Result<&'a [u8], Short> {
        let count = usize::try_from(u64::from_le_bytes(self.take()?)).map_err(|_| Short)?;
        let (taken, rest) = self.0.split_at_checked(count).ok_or(Short)?;
        self.0 = rest;
        Ok(taken)
    }
}

pub fn send(to: &mut impl Write, body: &[u8]) -> io::Result<()> {
    to.write_all(&(body.len() as u32).to_le_bytes())?;
    to.write_all(body)?;
    to.flush()
}

pub fn receive(from: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut size = [0; 4];
    from.read_exact(&mut size)?;
    let size = u32::from_le_bytes(size) as usize;
    if size > MOST {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("a message of {size} bytes is above {MOST}"),
        ));
    }
    let mut body = vec![0; size];
    from.read_exact(&mut body)?;
    Ok(body)
}
