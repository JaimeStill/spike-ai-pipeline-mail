//! The mbox reader: a store line, then each message in file order, read one at a time by
//! mail-parser's mbox iterator, so memory follows the message being read, not the file. A
//! message's id is its 1-based ordinal; no message has a folder. A message that fails the
//! header-shape check, or that mail-parser can't parse, gives a skipped line in its place, with a
//! warning, and the run goes on; a read that fails partway gives one for the message it was
//! reading, and ends there.
//!
//! The iterator reads mboxrd: a line quoted ">From ", ">>From " and so on loses one ">". An
//! mboxo is read the same way, so a body line that was ">From " to begin with loses its ">" too.
//! A message runs to the next line starting "From ", and mboxcl's Content-Length is not read, so
//! in an mboxcl or mboxcl2 an unquoted body line starting "From " starts a new message.

use std::fs::File;
use std::io::{BufReader, Write};
use std::path::Path;

use mail_parser::MessageBuffers;
use mail_parser::mailbox::mbox::MessageIterator;

use crate::files::Files;
use crate::jsonl::Lines;
use crate::model::{self, Skipped, Store};
use crate::{Fault, mime, sniff, warn, warn_below};

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
    let parser = mime::parser();
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
        let contents = unseparated(entry.contents());
        // A separator line with nothing after it is no message: it writes no line and no
        // warning, but keeps its ordinal, so the messages after it keep theirs.
        if contents.is_empty() {
            continue;
        }
        if !sniff::header_shape(contents).unwrap_or(false) {
            let reason = "it has no header block with a From or Date field".to_owned();
            skip(out, &place, ordinal, reason)?;
            continue;
        }
        let Some(parsed) = parser.parse_with(contents, &mut buffers) else {
            skip(out, &place, ordinal, "it can't be parsed".to_owned())?;
            continue;
        };
        let mut read = mime::message(&parsed.root(), files)?;
        read.message.id = Some(ordinal);
        warn_below(&place, read.notes);
        out.write(&read.message).map_err(Fault::Output)?;
        for (id, reason) in read.skipped {
            out.write(&Skipped {
                line: "skipped",
                kind: "attachment",
                folder: None,
                message: Some(ordinal),
                id: Some(id),
                reason,
            })
            .map_err(Fault::Output)?;
        }
        buffers = parsed.into_buffers();
    }
    Ok(())
}

/// A message's contents without the blank line that ends it, which the mbox writer adds before
/// the next separator line and after the last message, so the message is its source byte for
/// byte. Contents that don't end in a blank line are left as they are.
fn unseparated(contents: &[u8]) -> &[u8] {
    let Some(rest) = contents.strip_suffix(b"\n") else {
        return contents;
    };
    let rest = rest.strip_suffix(b"\r").unwrap_or(rest);
    if rest.is_empty() || rest.ends_with(b"\n") {
        rest
    } else {
        contents
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_blank_line_ending_a_message_is_the_separators() {
        for (contents, message) in [
            (
                &b"Subject: a\r\n\r\nbody\r\n\n"[..],
                &b"Subject: a\r\n\r\nbody\r\n"[..],
            ),
            (b"Subject: a\n\nbody\n\n", b"Subject: a\n\nbody\n"),
            (
                b"Subject: a\r\n\r\nbody\r\n\r\n",
                b"Subject: a\r\n\r\nbody\r\n",
            ),
            (b"Subject: a\n\nbody\n", b"Subject: a\n\nbody\n"),
            (b"Subject: a\n\nbody", b"Subject: a\n\nbody"),
            (b"\n", b""),
            (b"\r\n", b""),
            (b"", b""),
        ] {
            assert_eq!(unseparated(contents), message, "{contents:?}");
        }
    }
}
