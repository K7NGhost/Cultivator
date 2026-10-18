use std::io::{self, Read, Write};

#[cfg(test)]
mod tests;

pub(super) type Check<'a> = &'a (dyn Fn() -> bool + Sync);

pub(super) fn checkpoint(cancelled: Check<'_>) -> Result<(), String> {
    if cancelled() {
        Err("Plugin run cancelled.".into())
    } else {
        Ok(())
    }
}

pub(super) struct Reader<'a, R> {
    pub input: R,
    pub cancelled: Check<'a>,
}

impl<R: Read> Read for Reader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if (self.cancelled)() {
            return Err(io::Error::other("Plugin run cancelled."));
        }
        self.input.read(buffer)
    }
}

pub(super) fn copy(
    input: &mut impl Read,
    output: &mut impl Write,
    cancelled: Check<'_>,
) -> io::Result<u64> {
    let mut buffer = [0; 64 * 1024];
    let mut copied = 0;
    loop {
        if cancelled() {
            return Err(io::Error::other("Plugin run cancelled."));
        }
        let read = match input.read(&mut buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if read == 0 {
            return Ok(copied);
        }
        output.write_all(&buffer[..read])?;
        copied += read as u64;
    }
}
