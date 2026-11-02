//! String decoding by code page, through codepage-strings. PT_UNICODE is UTF-16LE, code page
//! 1200; PT_STRING8 is in the code page of its message, else of its store. A string that isn't
//! valid in its code page, or whose code page codepage-strings can't decode, is decoded lossily
//! and noted, and the caller turns each note into a warning.

use codepage_strings::Coding;

use crate::props::{Bag, Value, integer, tag};

/// UTF-16LE, the encoding of PT_UNICODE.
const UTF16LE: u16 = 1200;

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

    fn decode(&mut self, id: u16, bytes: &[u8], code_page: u16) -> String {
        match Coding::new(code_page) {
            Ok(coding) => match coding.decode(bytes) {
                Ok(text) => text.into_owned(),
                Err(_) => {
                    self.notes.push(format!(
                        "property 0x{id:04X} is not valid in code page {code_page}; decoded lossily"
                    ));
                    coding.decode_lossy(bytes).into_owned()
                }
            },
            Err(err) => {
                self.notes.push(format!(
                    "property 0x{id:04X}: code page {code_page} can't be decoded ({err}); \
                     decoded lossily as code page {FALLBACK}"
                ));
                Coding::new(FALLBACK)
                    .map(|coding| coding.decode_lossy(bytes).into_owned())
                    .unwrap_or_else(|_| String::from_utf8_lossy(bytes).into_owned())
            }
        }
    }
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
    fn the_message_code_page_falls_back_to_the_store() {
        let message = bag(tag::MESSAGE_CODEPAGE, Value::Integer(936));
        assert_eq!(message_code_page(&message, 1252), 936);
        assert_eq!(message_code_page(&Bag::new(), 1251), 1251);
        assert_eq!(store_code_page(&Bag::new()), FALLBACK);
        let store = bag(tag::CODE_PAGE_ID, Value::Integer(1250));
        assert_eq!(store_code_page(&store), 1250);
    }
}
