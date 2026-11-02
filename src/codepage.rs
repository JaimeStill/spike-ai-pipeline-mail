//! String decoding by code page, through codepage-strings. PT_UNICODE is UTF-16LE, code page
//! 1200; PT_STRING8 is in the code page of its message, else of its store. A string that isn't
//! valid in its code page, or whose code page codepage-strings can't decode, is decoded lossily
//! and noted, and the caller turns each note into a warning.

use codepage_strings::Coding;

use crate::props::{Bag, Value, integer, tag};

/// UTF-16LE, the encoding of PT_UNICODE.
const UTF16LE: u16 = 1200;

/// UTF-8.
pub const UTF8: u16 = 65001;

/// US-ASCII, which Outlook names for HTML bodies in plain ASCII and codepage-strings doesn't
/// know; [`decode`] reads it itself.
const US_ASCII: u16 = 20127;

/// Windows-1252, for PT_STRING8 when neither the message nor the store names a code page, and
/// for lossy decoding when the named code page can't be decoded at all.
pub const FALLBACK: u16 = 1252;

/// The store's code page: PidTagMessageCodepage, else PidTagCodePageId, on the store's own
/// properties, else [`FALLBACK`].
pub fn store_code_page(store: &Bag) -> u16 {
    code_page(store, tag::MESSAGE_CODEPAGE)
        .or_else(|| code_page(store, tag::CODE_PAGE_ID))
        .unwrap_or(FALLBACK)
}

/// The code page a message's PT_STRING8 values are in: its PidTagMessageCodepage, else the
/// store's.
pub fn message_code_page(message: &Bag, store: u16) -> u16 {
    code_page(message, tag::MESSAGE_CODEPAGE).unwrap_or(store)
}

fn code_page(bag: &Bag, id: u16) -> Option<u16> {
    integer(bag, id).and_then(|value| u16::try_from(value).ok())
}

/// Decodes one record's strings in one code page, and keeps a note for each lossy decoding.
pub struct Strings {
    code_page: u16,
    notes: Vec<String>,
}

impl Strings {
    pub fn new(code_page: u16) -> Self {
        Self {
            code_page,
            notes: Vec::new(),
        }
    }

    /// The text of a string property, or None when it is absent or not a string.
    pub fn text(&mut self, bag: &Bag, id: u16) -> Option<String> {
        match bag.get(&id)? {
            Value::Unicode(bytes) => Some(self.decode(id, bytes, UTF16LE)),
            Value::String8(bytes) => Some(self.decode(id, bytes, self.code_page)),
            _ => None,
        }
    }

    /// The notes of every lossy decoding so far, in the order they happened.
    pub fn into_notes(self) -> Vec<String> {
        self.notes
    }

    /// The text of `bytes` in `code_page`, with a note for property `id` when it is lossy.
    pub fn decode(&mut self, id: u16, bytes: &[u8], code_page: u16) -> String {
        let (text, lossy) = decode(bytes, code_page);
        if let Some(lossy) = lossy {
            self.notes.push(format!("property 0x{id:04X} {lossy}"));
        }
        text
    }
}

/// The text of `bytes` in `code_page`, and when the decoding is lossy, why, worded to follow
/// the name of what was decoded: "is not valid in code page 65001; decoded lossily".
pub fn decode(bytes: &[u8], code_page: u16) -> (String, Option<String>) {
    if code_page == US_ASCII {
        return ascii(bytes);
    }
    match Coding::new(code_page) {
        Ok(coding) => match coding.decode(bytes) {
            Ok(text) => (text.into_owned(), None),
            Err(_) => (
                coding.decode_lossy(bytes).into_owned(),
                Some(format!(
                    "is not valid in code page {code_page}; decoded lossily"
                )),
            ),
        },
        Err(err) => (
            Coding::new(FALLBACK)
                .map(|coding| coding.decode_lossy(bytes).into_owned())
                .unwrap_or_else(|_| String::from_utf8_lossy(bytes).into_owned()),
            Some(format!(
                "is in code page {code_page}, which can't be decoded ({err}); \
                 decoded lossily as code page {FALLBACK}"
            )),
        ),
    }
}

/// How far into an HTML document its `<meta>` charset is looked for, as the HTML standard's
/// prescan looks (HTML 13.2.3.2).
const PRESCAN: usize = 1024;

/// The code page of the charset an HTML document's own `<meta>` declares in its first
/// [`PRESCAN`] bytes, by a `charset` attribute or an `http-equiv` content type's `charset=`.
/// A label is read as the WHATWG Encoding Standard reads it; a UTF-16 label is UTF-8, as the
/// prescan has it, since bytes it can read as ASCII aren't UTF-16.
pub fn meta_charset(html: &[u8]) -> Option<u16> {
    let head = &html[..html.len().min(PRESCAN)];
    let mut at = 0;
    while at < head.len() {
        let rest = &head[at..];
        if rest.starts_with(b"<!--") {
            at += find(rest, b"-->").map_or(rest.len(), |end| end + 3);
            continue;
        }
        if rest.len() > 5
            && rest[..5].eq_ignore_ascii_case(b"<meta")
            && matches!(rest[5], b' ' | b'\t' | b'\n' | b'\r' | b'\x0C' | b'/')
        {
            let (label, end) = meta_label(&rest[5..]);
            if let Some(code_page) = label.and_then(label_code_page) {
                return Some(code_page);
            }
            at += 5 + end;
            continue;
        }
        at += 1;
    }
    None
}

/// The charset label of one `<meta>` tag's attributes, and how far they run.
fn meta_label(tag: &[u8]) -> (Option<&[u8]>, usize) {
    let (mut charset, mut content, mut pragma) = (None, None, false);
    let mut at = 0;
    loop {
        while tag
            .get(at)
            .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'/')
        {
            at += 1;
        }
        let Some(&byte) = tag.get(at) else { break };
        if byte == b'>' {
            at += 1;
            break;
        }
        let name_start = at;
        while tag
            .get(at)
            .is_some_and(|b| !b.is_ascii_whitespace() && !matches!(b, b'=' | b'>' | b'/'))
        {
            at += 1;
        }
        let name = &tag[name_start..at];
        while tag.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        let mut value: &[u8] = b"";
        if tag.get(at) == Some(&b'=') {
            at += 1;
            while tag.get(at).is_some_and(u8::is_ascii_whitespace) {
                at += 1;
            }
            match tag.get(at) {
                Some(&quote @ (b'"' | b'\'')) => {
                    let start = at + 1;
                    let end = tag[start..]
                        .iter()
                        .position(|b| *b == quote)
                        .map_or(tag.len(), |end| start + end);
                    value = &tag[start..end];
                    at = (end + 1).min(tag.len());
                }
                _ => {
                    let start = at;
                    while tag
                        .get(at)
                        .is_some_and(|b| !b.is_ascii_whitespace() && *b != b'>')
                    {
                        at += 1;
                    }
                    value = &tag[start..at];
                }
            }
        }
        if name.eq_ignore_ascii_case(b"charset") {
            charset.get_or_insert(value);
        } else if name.eq_ignore_ascii_case(b"content") {
            content.get_or_insert(value);
        } else if name.eq_ignore_ascii_case(b"http-equiv") {
            pragma |= value.eq_ignore_ascii_case(b"content-type");
        }
    }
    let label = charset.or_else(|| {
        let content = content.filter(|_| pragma)?;
        content_charset(content)
    });
    (label, at)
}

/// The value of `charset=` in a content type, as the HTML standard extracts it.
fn content_charset(content: &[u8]) -> Option<&[u8]> {
    let lower = content.to_ascii_lowercase();
    let at = find(&lower, b"charset")? + 7;
    let rest = &content[at..];
    let rest = rest
        .trim_ascii_start()
        .strip_prefix(b"=")?
        .trim_ascii_start();
    let (value, _) = match rest.first() {
        Some(&quote @ (b'"' | b'\'')) => {
            let rest = &rest[1..];
            rest.split_at(rest.iter().position(|b| *b == quote)?)
        }
        _ => rest.split_at(
            rest.iter()
                .position(|b| b.is_ascii_whitespace() || *b == b';')
                .unwrap_or(rest.len()),
        ),
    };
    (!value.is_empty()).then_some(value)
}

fn label_code_page(label: &[u8]) -> Option<u16> {
    let encoding = encoding_rs::Encoding::for_label(label)?;
    let encoding = if encoding == encoding_rs::UTF_16LE || encoding == encoding_rs::UTF_16BE {
        encoding_rs::UTF_8
    } else {
        encoding
    };
    codepage::from_encoding(encoding)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// `bytes` as US-ASCII: a byte past 0x7F is U+FFFD, and makes the decoding lossy.
fn ascii(bytes: &[u8]) -> (String, Option<String>) {
    let text = bytes
        .iter()
        .map(|&byte| {
            if byte.is_ascii() {
                char::from(byte)
            } else {
                '\u{FFFD}'
            }
        })
        .collect();
    let lossy = (!bytes.is_ascii())
        .then(|| format!("is not valid in code page {US_ASCII}; decoded lossily"));
    (text, lossy)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bag(id: u16, value: Value) -> Bag {
        Bag::from([(id, value)])
    }

    #[test]
    fn unicode_is_utf16le() {
        let bytes: Vec<u8> = "Jörn".encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut strings = Strings::new(FALLBACK);
        let text = strings.text(&bag(tag::SUBJECT, Value::Unicode(bytes)), tag::SUBJECT);
        assert_eq!(text.as_deref(), Some("Jörn"));
        assert!(strings.into_notes().is_empty());
    }

    #[test]
    fn string8_is_decoded_by_the_code_page() {
        let mut strings = Strings::new(1251);
        let value = Value::String8(vec![0xCF, 0xF0, 0xE8]);
        let text = strings.text(&bag(tag::SUBJECT, value), tag::SUBJECT);
        assert_eq!(text.as_deref(), Some("При"));
    }

    #[test]
    fn invalid_bytes_decode_lossily_with_a_note() {
        let mut strings = Strings::new(65001);
        let value = Value::String8(vec![b'a', 0xFF, b'b']);
        let text = strings.text(&bag(tag::SUBJECT, value), tag::SUBJECT);
        assert_eq!(text.as_deref(), Some("a\u{FFFD}b"));
        assert_eq!(
            strings.into_notes(),
            ["property 0x0037 is not valid in code page 65001; decoded lossily"]
        );
    }

    #[test]
    fn an_unknown_code_page_decodes_lossily_as_the_fallback() {
        let mut strings = Strings::new(1);
        let value = Value::String8(vec![b'a', 0xE9]);
        let text = strings.text(&bag(tag::SUBJECT, value), tag::SUBJECT);
        assert_eq!(text.as_deref(), Some("aé"));
        assert_eq!(strings.into_notes().len(), 1);
    }

    #[test]
    fn us_ascii_is_read_though_codepage_strings_lacks_it() {
        assert_eq!(
            decode(b"<p>Hi</p>", US_ASCII),
            ("<p>Hi</p>".to_owned(), None)
        );
        let (text, lossy) = decode(b"a\xE9", US_ASCII);
        assert_eq!(text, "a\u{FFFD}");
        assert_eq!(
            lossy.as_deref(),
            Some("is not valid in code page 20127; decoded lossily")
        );
    }

    #[test]
    fn a_meta_charset_names_the_code_page() {
        let cases: &[(&[u8], Option<u16>)] = &[
            (b"<html><head><meta charset=\"gb2312\"></head>", Some(936)),
            (b"<META charset=Shift_JIS>", Some(932)),
            (
                b"<meta http-equiv=\"Content-Type\" content=\"text/html; charset=windows-1251\">",
                Some(1251),
            ),
            (
                b"<META HTTP-EQUIV='Content-Type' CONTENT='text/html; charset=\"utf-8\"'>",
                Some(65001),
            ),
            (b"<meta charset=utf-16>", Some(65001)),
            // ISO-8859-1 is windows-1252 to the Encoding Standard.
            (b"<meta charset=iso-8859-1>", Some(1252)),
            // A content type without the pragma names no charset.
            (b"<meta content=\"text/html; charset=koi8-r\">", None),
            (b"<!-- <meta charset=big5> --><p>hi</p>", None),
            (
                b"<meta charset=no-such-charset><meta charset=big5>",
                Some(950),
            ),
            (b"<metadata charset=big5>", None),
            (b"<p>no meta</p>", None),
        ];
        for (html, code_page) in cases {
            assert_eq!(
                meta_charset(html),
                *code_page,
                "{}",
                String::from_utf8_lossy(html)
            );
        }
        let mut late = vec![b' '; PRESCAN];
        late.extend_from_slice(b"<meta charset=big5>");
        assert_eq!(meta_charset(&late), None);
    }

    #[test]
    fn the_message_code_page_falls_back_to_the_store() {
        let message = bag(tag::MESSAGE_CODEPAGE, Value::Integer(936));
        assert_eq!(message_code_page(&message, 1252), 936);
        assert_eq!(message_code_page(&Bag::new(), 1251), 1251);
        assert_eq!(store_code_page(&Bag::new()), FALLBACK);
        let store = bag(tag::CODE_PAGE_ID, Value::Integer(1250));
        assert_eq!(store_code_page(&store), 1250);
    }
}
