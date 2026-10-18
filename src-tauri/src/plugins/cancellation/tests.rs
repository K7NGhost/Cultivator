use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn large_copies_stop_between_chunks() {
    let flag = AtomicBool::new(false);
    struct Input<'a>(&'a AtomicBool);
    impl Read for Input<'_> {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            bytes.fill(42);
            self.0.store(true, Ordering::Release);
            Ok(bytes.len())
        }
    }
    let mut output = Vec::new();
    assert!(copy(&mut Input(&flag), &mut output, &|| flag
        .load(Ordering::Acquire))
    .is_err());
    assert_eq!(output.len(), 64 * 1024);
    let mut reader = Reader {
        input: &b"original"[..],
        cancelled: &|| true,
    };
    assert!(reader.read(&mut [0; 8]).is_err());
}
