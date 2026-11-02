//! The JSON Lines writer: one serialized line per call, newline-terminated, and nothing else.

use std::io::{self, Write};

use serde::Serialize;

pub struct Lines<W: Write> {
    out: W,
}

impl<W: Write> Lines<W> {
    pub fn new(out: W) -> Self {
        Self { out }
    }

    pub fn write(&mut self, line: &impl Serialize) -> io::Result<()> {
        serde_json::to_writer(&mut self.out, line)?;
        self.out.write_all(b"\n")
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}
