//! The guard every reader call runs inside, in every reader: a reader's error, or its panic,
//! becomes the reason of one record's failure, and the walk goes on.

use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::panic_text;

/// Why one read failed, and whether by a panic. After a panic, a reader's state (outlook-pst's
/// file mutex, cfb's allocator lock) may be poisoned, so the caller reopens its input.
pub struct Failure {
    pub reason: String,
    pub panicked: bool,
}

/// Runs one read inside a reader, turning its error or its panic into a reason.
pub fn guarded<T>(read: impl FnOnce() -> io::Result<T>) -> Result<T, Failure> {
    match catch_unwind(AssertUnwindSafe(read)) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => Err(Failure {
            reason: err.to_string(),
            panicked: false,
        }),
        Err(payload) => Err(Failure {
            reason: format!("the reader panicked: {}", panic_text(&*payload)),
            panicked: true,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reader_panic_becomes_a_reason() {
        let failure = guarded::<()>(|| panic!("index out of bounds"))
            .err()
            .unwrap();
        assert_eq!(failure.reason, "the reader panicked: index out of bounds");
        assert!(failure.panicked);
    }

    #[test]
    fn a_reader_error_becomes_a_reason() {
        let failure = guarded::<()>(|| Err(io::Error::other("Missing Sub-Node Tree on message")))
            .err()
            .unwrap();
        assert_eq!(failure.reason, "Missing Sub-Node Tree on message");
        assert!(!failure.panicked);
    }
}
