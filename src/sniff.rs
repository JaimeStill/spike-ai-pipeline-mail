//! The input's format, from its content and never from its name. The binary formats are told
//! first, by their magic: a .pst, an .ost and a .msg's compound file. Then the text formats: an
//! mbox by its first line, and an .eml by the shape of its header block ([`header_shape`]).

use std::io::{self, BufRead, Read};

/// What the first bytes say the input is.
#[derive(Debug, PartialEq)]
pub enum Format {
    /// A PST store, with the encoding its version names.
    Pst(Encoding),
    /// A PST header with client magic "SO": an .ost, a server mailbox's offline cache.
    Ost,
    /// A PST store of a version outlook-pst doesn't read, such as 36, the 4K-page format.
    PstVersion(u16),
    /// An OLE2 compound file, the container of a .msg.
    CompoundFile,
    /// A first line that starts "From ", an mbox's separator line.
    Mbox,
    /// None of these by its first bytes: an .eml if its header block has the shape of one.
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Encoding {
    Unicode,
    Ansi,
}

impl Encoding {
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Unicode => "unicode",
            Encoding::Ansi => "ansi",
        }
    }
}

/// The bytes [`sniff`] needs.
pub const PREFIX: usize = 12;

/// HEADER.dwMagic (MS-PST 2.2.2.6).
const PST_MAGIC: &[u8] = b"!BDN";
/// HEADER.wMagicClient of a .pst and of an .ost.
const PST_CLIENT: &[u8] = b"SM";
const OST_CLIENT: &[u8] = b"SO";
/// The compound file header signature (MS-CFB 2.2).
const CFB_MAGIC: &[u8] = &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
/// The start of an mbox's separator line.
const MBOX_SEPARATOR: &[u8] = b"From ";

pub fn sniff(prefix: &[u8]) -> Format {
    if prefix.starts_with(CFB_MAGIC) {
        return Format::CompoundFile;
    }
    if prefix.starts_with(PST_MAGIC) {
        return pst(prefix);
    }
    if prefix.starts_with(MBOX_SEPARATOR) {
        return Format::Mbox;
    }
    Format::Other
}

/// A prefix that starts with the PST magic: a .pst of a version it names, an .ost, or, when
/// it is short or its client magic is neither, [`Format::Other`].
fn pst(prefix: &[u8]) -> Format {
    if prefix.len() < PREFIX {
        return Format::Other;
    }
    let client = &prefix[8..10];
    if client == OST_CLIENT {
        return Format::Ost;
    }
    if client != PST_CLIENT {
        return Format::Other;
    }
    // HEADER.wVer: 14 and 15 are ANSI, 23 is Unicode; outlook-pst reads no other.
    match u16::from_le_bytes([prefix[10], prefix[11]]) {
        14 | 15 => Format::Pst(Encoding::Ansi),
        23 => Format::Pst(Encoding::Unicode),
        version => Format::PstVersion(version),
    }
}

/// The longest line [`header_shape`] reads; a longer one ends the header block. RFC 5322 2.1.1
/// caps a line at 998 characters, which real mail sometimes passes, though not by this much.
const LINE_MAX: u64 = 64 * 1024;

/// Whether `input` has the shape of an RFC 5322 message: its first line a header field, and a
/// From or Date field in its header block. The block runs to its blank line, to the end of the
/// input, or to the first line that is neither a field nor a field's folded continuation. Lines
/// are read one at a time, so a large input that is no mail is told without being held.
pub fn header_shape(mut input: impl BufRead) -> io::Result<bool> {
    let mut line = Vec::new();
    let mut first = true;
    loop {
        line.clear();
        (&mut input).take(LINE_MAX).read_until(b'\n', &mut line)?;
        let content = line.strip_suffix(b"\n").unwrap_or(&line);
        let content = content.strip_suffix(b"\r").unwrap_or(content);
        if content.is_empty() {
            return Ok(false);
        }
        let folded = matches!(content[0], b' ' | b'\t');
        match field_name(content) {
            Some(name)
                if name.eq_ignore_ascii_case(b"From") || name.eq_ignore_ascii_case(b"Date") =>
            {
                return Ok(true);
            }
            Some(_) => {}
            None if folded && !first => {}
            None => return Ok(false),
        }
        if !line.ends_with(b"\n") {
            // The end of the input, or a line past LINE_MAX.
            return Ok(false);
        }
        first = false;
    }
}

/// The name of the header field `line` starts, if it starts one: printable ASCII but the colon
/// (RFC 5322 3.6.8's ftext), then the colon, after any spaces or tabs, as RFC 5322 4.5.8's
/// obsolete syntax allows.
fn field_name(line: &[u8]) -> Option<&[u8]> {
    let end = line
        .iter()
        .position(|&byte| !matches!(byte, 33..=57 | 59..=126))?;
    let rest = line[end..]
        .iter()
        .find(|&&byte| !matches!(byte, b' ' | b'\t'))?;
    (end > 0 && *rest == b':').then_some(&line[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_block_with_from_or_date_is_header_shaped() {
        for input in [
            &b"From: ann@example.com\r\n\r\nbody"[..],
            b"Received: from a\r\n by b; Mon, 12 Jan 2026 14:16:30 +0000\r\nDate: x\r\n",
            b"Subject : hi\nfrom: ann@example.com\n",
            b"date:x",
        ] {
            assert!(header_shape(input).unwrap(), "{input:?}");
        }
    }

    #[test]
    fn anything_else_is_not_header_shaped() {
        for input in [
            &b""[..],
            b"\r\nFrom: ann@example.com\r\n",
            b" From: ann@example.com\r\n",
            b"Subject: hi\r\n\r\nFrom: ann@example.com\r\n",
            b"Subject: hi\r\nnot a field\r\nFrom: ann@example.com\r\n",
            b"Subject: hi\r\nTo: bob@example.com\r\n",
            b": no name\r\nFrom: ann@example.com\r\n",
            b"From ann@example.com Sat Jan  3 01:05:34 1996\n",
        ] {
            assert!(!header_shape(input).unwrap(), "{input:?}");
        }
    }

    #[test]
    fn an_mbox_is_told_by_its_separator_line_not_a_from_field() {
        assert_eq!(sniff(b"From ann@example.com"), Format::Mbox);
        assert_eq!(sniff(b"From: ann@example.com"), Format::Other);
    }
}
