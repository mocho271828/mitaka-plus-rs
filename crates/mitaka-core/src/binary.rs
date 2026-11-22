use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

pub(crate) fn read_bounded(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let expected = file.metadata()?.len();
    if expected > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file exceeds byte limit",
        ));
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "input changed during read",
        ));
    }
    Ok(bytes)
}

pub(crate) fn at_path<T>(path: &Path, result: io::Result<T>) -> io::Result<T> {
    result.map_err(|error| io::Error::new(error.kind(), format!("{}: {error}", path.display())))
}

pub(crate) struct Reader<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) offset: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    pub(crate) fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }
    pub(crate) fn invalid(&self, reason: &str) -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("byte {}: {reason}", self.offset),
        )
    }
    pub(crate) fn take(&mut self, len: usize) -> io::Result<&'a [u8]> {
        if len > self.remaining() {
            return Err(self.invalid("truncated input"));
        }
        let start = self.offset;
        self.offset += len;
        Ok(&self.bytes[start..self.offset])
    }
    pub(crate) fn array<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        Ok(self
            .take(N)?
            .try_into()
            .expect("take returned the exact requested length"))
    }
    pub(crate) fn u8(&mut self) -> io::Result<u8> {
        Ok(self.array::<1>()?[0])
    }
    pub(crate) fn i16(&mut self) -> io::Result<i16> {
        Ok(i16::from_le_bytes(self.array()?))
    }
    pub(crate) fn i32(&mut self) -> io::Result<i32> {
        Ok(i32::from_le_bytes(self.array()?))
    }
    pub(crate) fn f32(&mut self) -> io::Result<f32> {
        let v = f32::from_le_bytes(self.array()?);
        if v.is_finite() {
            Ok(v)
        } else {
            Err(self.invalid("non-finite floating-point value"))
        }
    }
    pub(crate) fn name(&mut self) -> io::Result<Vec<u8>> {
        let len = self.u8()?;
        Ok(self.take(usize::from(len))?.to_vec())
    }
}
