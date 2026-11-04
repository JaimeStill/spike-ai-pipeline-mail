//! The mbox reader: a store line, then each message in file order, read one at a time by
//! mail-parser's mbox iterator, which unquotes ">From " lines as mboxrd does. A message's id is
//! its 1-based ordinal; no message has a folder. A message that fails the header-shape check, or
//! that mail-parser can't parse, gives a skipped line in its place, with a warning, and the run
//! goes on; a read that fails partway gives one for the message it was reading, and ends there.

use std::fs::File;
use std::io::{BufReader, Write};
use std::path::Path;

use mail_parser::mailbox::mbox::MessageIterator;
use mail_parser::{MessageBuffers, MessageParser};

use crate::files::Files;
use crate::jsonl::Lines;
use crate::model::{self, Skipped, Store};
use crate::{Fault, mime, sniff, warn, warn_all};

/// Reads the mbox at `path` and writes its store line and its messages' lines to `out`.
pub fn read(path: &Path, out: &mut Lines<impl Write>, files: &mut Files) -> Result<(), Fault> {
    let file =
        File::open(path).map_err(|err| Fault::Refused(format!("{}: {err}", path.display())))?;
    files.create()?;
    out.write(&Store {
        line: "store",
        schema: model::SCHEMA,
        format: "mbox",
        encoding: None,
        password: None,
        display_name: None,
    })
    .map_err(Fault::Output)?;
    let parser = MessageParser::new();
    let mut buffers = MessageBuffers::new();
    for (ordinal, entry) in (1u32..).zip(MessageIterator::new(BufReader::new(file))) {
        let place = format!("message {ordinal}");
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                skip(
                    out,
                    &place,
                    ordinal,
                    format!("the mbox can't be read: {err}"),
                )?;
                break;
            }
        };
        let contents = entry.contents();
        if !sniff::header_shape(contents).unwrap_or(false) {
            let reason = "it has no header block with a From or Date field".to_owned();
            skip(out, &place, ordinal, reason)?;
            continue;
        }
        let Some(parsed) = parser.parse_with(contents, &mut buffers) else {
            skip(out, &place, ordinal, "it can't be parsed".to_owned())?;
            continue;
        };
        let (mut message, notes) = mime::message(&parsed.root());
        message.id = Some(ordinal);
        warn_all(&place, notes);
        out.write(&message).map_err(Fault::Output)?;
        buffers = parsed.into_buffers();
    }
    Ok(())
}

/// Writes a message's skipped line, and its warning.
fn skip(
    out: &mut Lines<impl Write>,
    place: &str,
    ordinal: u32,
    reason: String,
) -> Result<(), Fault> {
    warn(place, &format!("skipped: {reason}"));
    out.write(&Skipped {
        line: "skipped",
        kind: "message",
        folder: None,
        message: None,
        id: Some(ordinal),
        reason,
    })
    .map_err(Fault::Output)
}
