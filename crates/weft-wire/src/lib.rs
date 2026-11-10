pub mod generate;

use std::io::{self, Read, Write};

pub const JOIN: u16 = u16::MAX;
pub const TURN: u16 = u16::MAX - 1;
pub const DONE: u16 = u16::MAX - 2;
pub const REFUSED: i32 = i32::MIN;
const MOST: usize = 1 << 28;

#[derive(Debug)]
pub struct Short;

pub trait Wire: Sized {
    fn put(self, to: &mut Vec<u8>);
    fn get(from: &mut Reader) -> Result<Self, Short>;
}

macro_rules! wire {
    ($($t:ty)*) => {$(
        impl Wire for $t {
            fn put(self, to: &mut Vec<u8>) {
                to.extend_from_slice(&self.to_le_bytes());
            }

            fn get(from: &mut Reader) -> Result<Self, Short> {
                from.take(size_of::<$t>())?
                    .try_into()
                    .map(<$t>::from_le_bytes)
                    .map_err(|_| Short)
            }
        }
    )*};
}

wire!(u8 u16 u32 i32 u64 i64 f64);

impl Wire for bool {
    fn put(self, to: &mut Vec<u8>) {
        to.push(u8::from(self));
    }

    fn get(from: &mut Reader) -> Result<Self, Short> {
        u8::get(from).map(|b| b != 0)
    }
}

impl Wire for usize {
    fn put(self, to: &mut Vec<u8>) {
        (self as u64).put(to);
    }

    fn get(from: &mut Reader) -> Result<Self, Short> {
        u64::get(from).and_then(|n| usize::try_from(n).map_err(|_| Short))
    }
}

#[derive(Default)]
pub struct Writer(pub Vec<u8>);

impl Writer {
    pub fn put<T: Wire>(&mut self, value: T) {
        value.put(&mut self.0);
    }

    pub fn bytes(&mut self, value: &[u8]) {
        self.put(value.len());
        self.0.extend_from_slice(value);
    }

    pub fn opt(&mut self, value: Option<&[u8]>) {
        self.put(value.is_some());
        if let Some(value) = value {
            self.bytes(value);
        }
    }
}

pub struct Reader<'a>(pub &'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], Short> {
        let (taken, rest) = self.0.split_at_checked(count).ok_or(Short)?;
        self.0 = rest;
        Ok(taken)
    }

    pub fn get<T: Wire>(&mut self) -> Result<T, Short> {
        T::get(self)
    }

    pub fn bytes(&mut self) -> Result<&'a [u8], Short> {
        let count = self.get::<usize>()?;
        self.take(count)
    }

    pub fn opt(&mut self) -> Result<Option<&'a [u8]>, Short> {
        match self.get::<bool>()? {
            true => self.bytes().map(Some),
            false => Ok(None),
        }
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
