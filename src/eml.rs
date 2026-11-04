//! The .eml reader: one RFC 5322 message, parsed whole by mail-parser and mapped by
//! [`crate::mime`]. The input has passed the header-shape check before it gets here; one that
//! mail-parser still can't parse, or one of 4 GiB or more, which it never parses, is refused.

use std::io::Write;
use std::path::Path;

use mail_parser::MessageParser;

use crate::files::Files;
use crate::jsonl::Lines;
use crate::model::{self, Store};
use crate::{Fault, mime, warn_all};

/// The size mail-parser parses up to: its offsets are 32-bit.
const SIZE_MAX: u64 = 1 << 32;

/// Reads the .eml at `path` and writes its store line and its message line to `out`.
pub fn read(path: &Path, out: &mut Lines<impl Write>, files: &mut Files) -> Result<(), Fault> {
    let input = path.display();
    let refused = |err: std::io::Error| Fault::Refused(format!("{input}: {err}"));
    let size = std::fs::metadata(path).map_err(refused)?.len();
    if size >= SIZE_MAX {
        return Err(Fault::Refused(format!(
            "{input}: an .eml of 4 GiB or more is not read"
        )));
    }
    let raw = std::fs::read(path).map_err(refused)?;
    let Some(parsed) = MessageParser::new().parse(&raw) else {
        return Err(Fault::Refused(format!(
            "{input}: the message can't be parsed"
        )));
    };
    files.create()?;
    out.write(&Store {
        line: "store",
        schema: model::SCHEMA,
        format: "eml",
        encoding: None,
        password: None,
        display_name: None,
    })
    .map_err(Fault::Output)?;
    let (message, notes) = mime::message(&parsed.root());
    warn_all("message", notes);
    out.write(&message).map_err(Fault::Output)
}
