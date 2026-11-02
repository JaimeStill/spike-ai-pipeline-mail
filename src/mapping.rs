//! The one mapping from MAPI properties to the message model, shared by the .pst and .msg
//! readers. It reads only what a reader has put in the bags, and returns its notes (lossy
//! decodings, values it couldn't use) for the caller to warn about with the record's place.

use sha2::{Digest, Sha256};

use crate::codepage::{self, Strings};
use crate::model::{Address, Attachment, Body, Derived, Message};
use crate::props::{Bag, Value, integer, tag};
use crate::{markdown, rtf};

/// Recipient types of PidTagRecipientType (MS-OXOMSG 2.2.3.1), less the MAPI_P1 resend flag.
const MAPI_TO: i64 = 1;
const MAPI_CC: i64 = 2;
const MAPI_BCC: i64 = 3;
const MAPI_P1: i64 = 0x1000_0000;

/// Maps a message's properties and its recipient table's rows to a message line. Strings in
/// PT_STRING8 are decoded in `code_page`.
pub fn message(props: &Bag, recipients: &[Bag], code_page: u16) -> (Message, Vec<String>) {
    let mut strings = Strings::new(code_page);
    let mut notes = Vec::new();
    let mut message = Message {
        line: "message",
        class: strings.text(props, tag::MESSAGE_CLASS),
        subject: strings.text(props, tag::SUBJECT).map(strip_subject_prefix),
        sender: address(
            &mut strings,
            props,
            tag::SENDER_NAME,
            tag::SENDER_EMAIL_ADDRESS,
            tag::SENDER_ADDRESS_TYPE,
        ),
        submit_time: time(props, tag::CLIENT_SUBMIT_TIME, &mut notes),
        delivery_time: time(props, tag::MESSAGE_DELIVERY_TIME, &mut notes),
        creation_time: time(props, tag::CREATION_TIME, &mut notes),
        modification_time: time(props, tag::LAST_MODIFICATION_TIME, &mut notes),
        importance: named(
            props,
            tag::IMPORTANCE,
            &["low", "normal", "high"],
            &mut notes,
        ),
        sensitivity: named(
            props,
            tag::SENSITIVITY,
            &["normal", "personal", "private", "confidential"],
            &mut notes,
        ),
        transport_headers: strings.text(props, tag::TRANSPORT_MESSAGE_HEADERS),
        message_id: strings.text(props, tag::INTERNET_MESSAGE_ID),
        in_reply_to: strings.text(props, tag::IN_REPLY_TO_ID),
        references: strings.text(props, tag::INTERNET_REFERENCES),
        conversation_topic: strings.text(props, tag::CONVERSATION_TOPIC),
        conversation_index: binary(props, tag::CONVERSATION_INDEX).map(hex),
        body: body(&mut strings, props, code_page, &mut notes),
        ..Message::default()
    };
    for (row, recipient) in recipients.iter().enumerate() {
        let Some(address) = address(
            &mut strings,
            recipient,
            tag::DISPLAY_NAME,
            tag::EMAIL_ADDRESS,
            tag::ADDRESS_TYPE,
        ) else {
            continue;
        };
        match integer(recipient, tag::RECIPIENT_TYPE).map(|kind| kind & !MAPI_P1) {
            Some(MAPI_TO) => message.to.push(address),
            Some(MAPI_CC) => message.cc.push(address),
            Some(MAPI_BCC) => message.bcc.push(address),
            kind => notes.push(format!(
                "recipient {row} has recipient type {kind:?}, not to, cc or bcc; left out"
            )),
        }
    }
    let mut all = strings.into_notes();
    all.append(&mut notes);
    (message, all)
}

/// A message's bodies: the stored plain text, HTML and RTF, then the HTML recovered from the
/// RTF and the Markdown made from the HTML, else from the plain text. A body that can't be
/// read is left out with a note, and the message keeps the rest.
fn body(strings: &mut Strings, props: &Bag, code_page: u16, notes: &mut Vec<String>) -> Body {
    let text = strings.text(props, tag::BODY);
    let html = html(strings, props, code_page);
    let rtf = binary(props, tag::RTF_COMPRESSED).and_then(|compressed| {
        rtf::decompress(compressed)
            .map_err(|reason| {
                notes.push(format!(
                    "property 0x{:04X} {reason}; left out",
                    tag::RTF_COMPRESSED
                ));
            })
            .ok()
    });
    let from_rtf = rtf.as_deref().and_then(rtf::html).map(|(html, mut lossy)| {
        notes.append(&mut lossy);
        html
    });
    let markdown = match (&html, &from_rtf, &text) {
        (Some(html), _, _) | (None, Some(html), _) => Some(markdown::from_html(html)),
        (None, None, Some(text)) => Some(markdown::from_text(text)),
        (None, None, None) => None,
    }
    .and_then(|made| {
        made.map_err(|reason| notes.push(format!("the Markdown body {reason}; left out")))
            .ok()
    });
    Body {
        text,
        html,
        rtf,
        derived: Derived {
            html: from_rtf,
            markdown,
        },
    }
}

/// The stored HTML body. PidTagHtml is PT_BINARY, decoded in the code page
/// PidTagInternetCodepage names, else the one the HTML's own `<meta>` declares, else UTF-8 when
/// the bytes are valid UTF-8, else the message's, which is the store's when the message names
/// none. As PidTagBodyHtml, a string, it is decoded as any string is.
fn html(strings: &mut Strings, props: &Bag, code_page: u16) -> Option<String> {
    let Some(bytes) = binary(props, tag::HTML) else {
        return strings.text(props, tag::HTML);
    };
    let code_page = integer(props, tag::INTERNET_CODEPAGE)
        .and_then(|value| u16::try_from(value).ok())
        .or_else(|| codepage::meta_charset(bytes))
        .or_else(|| std::str::from_utf8(bytes).is_ok().then_some(codepage::UTF8))
        .unwrap_or(code_page);
    Some(strings.decode(tag::HTML, bytes, code_page))
}

/// ATT_MHTML_REF of PidTagAttachFlags (MS-OXCMSG 2.2.2.18): the HTML body refers to the
/// attachment by its content-id or location, which is how an inline disposition is stored.
const ATT_MHTML_REF: i64 = 0x4;

/// The longest stored extension a file name keeps.
const EXTENSION_MAX: usize = 8;

/// Maps an attachment's properties, and its content when it was read, to an attachment record.
/// Strings in PT_STRING8 are decoded in `code_page`, the message's.
pub fn attachment(
    props: &Bag,
    content: Option<&[u8]>,
    code_page: u16,
) -> (Attachment, Vec<String>) {
    let mut strings = Strings::new(code_page);
    let mut notes = Vec::new();
    let filename = strings
        .text(props, tag::ATTACH_LONG_FILENAME)
        .or_else(|| strings.text(props, tag::ATTACH_FILENAME));
    // The stored extension is PidTagAttachExtension, else the filename's.
    let extension = strings.text(props, tag::ATTACH_EXTENSION).or_else(|| {
        let name = filename.as_deref()?;
        name.rsplit_once('.')
            .map(|(_, extension)| extension.to_owned())
    });
    let attachment = Attachment {
        file: content.map(|content| file_name(content, extension.as_deref())),
        filename,
        mime_type: strings.text(props, tag::ATTACH_MIME_TAG),
        content_id: strings.text(props, tag::ATTACH_CONTENT_ID),
        inline: integer(props, tag::ATTACH_FLAGS).is_some_and(|flags| flags & ATT_MHTML_REF != 0),
        method: method(props, &mut notes),
        size: content.map(|content| content.len() as u64),
    };
    let mut all = strings.into_notes();
    all.append(&mut notes);
    (attachment, all)
}

/// A content's file name: the lowercase hex SHA-256 of its bytes, plus `extension`, with or
/// without its leading dot, when it is 1 to [`EXTENSION_MAX`] ASCII letters and digits. Any
/// other extension is left off, so a stored name never puts a path or an odd character in the
/// file name. The extension keeps its stored case, as office's media names do.
pub fn file_name(content: &[u8], extension: Option<&str>) -> String {
    let mut name = hex(&Sha256::digest(content));
    let extension = extension.map(|extension| extension.strip_prefix('.').unwrap_or(extension));
    if let Some(extension) = extension
        && (1..=EXTENSION_MAX).contains(&extension.len())
        && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        name.push('.');
        name.push_str(extension);
    }
    name
}

/// PidTagAttachMethod (MS-OXCMSG 2.2.2.9) by name.
fn method(props: &Bag, notes: &mut Vec<String>) -> Option<&'static str> {
    let value = integer(props, tag::ATTACH_METHOD)?;
    let name = match value {
        0 => "none",
        1 => "by_value",
        2 => "by_reference",
        4 => "by_reference_only",
        5 => "embedded_message",
        6 => "storage",
        7 => "by_web_reference",
        _ => {
            notes.push(format!(
                "property 0x{:04X} has unknown value {value}; left out",
                tag::ATTACH_METHOD
            ));
            return None;
        }
    };
    Some(name)
}

/// A folder's display name and container class, decoded in `code_page`.
pub fn folder(props: &Bag, code_page: u16) -> (Option<String>, Option<String>, Vec<String>) {
    let mut strings = Strings::new(code_page);
    let name = strings.text(props, tag::DISPLAY_NAME);
    let class = strings.text(props, tag::CONTAINER_CLASS);
    (name, class, strings.into_notes())
}

/// A store's display name, decoded in `code_page`.
pub fn store_name(props: &Bag, code_page: u16) -> (Option<String>, Vec<String>) {
    let mut strings = Strings::new(code_page);
    let name = strings.text(props, tag::DISPLAY_NAME);
    (name, strings.into_notes())
}

fn address(strings: &mut Strings, bag: &Bag, name: u16, email: u16, kind: u16) -> Option<Address> {
    let address = Address {
        name: strings.text(bag, name),
        address: strings.text(bag, email),
        address_type: strings.text(bag, kind),
    };
    (!address.is_empty()).then_some(address)
}

/// A .pst stores PidTagSubject with a two-character prefix when its first character is U+0001
/// (MS-PST 2.5.3.1.1.1): the marker and the prefix's length. Neither is part of the subject.
fn strip_subject_prefix(subject: String) -> String {
    let mut chars = subject.chars();
    if chars.next() == Some('\u{1}') {
        chars.next();
        chars.as_str().to_owned()
    } else {
        subject
    }
}

fn binary(bag: &Bag, id: u16) -> Option<&[u8]> {
    match bag.get(&id)? {
        Value::Binary(bytes) => Some(bytes),
        _ => None,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// An enumerated integer property as its name, when the value is one of `names`.
fn named(
    bag: &Bag,
    id: u16,
    names: &[&'static str],
    notes: &mut Vec<String>,
) -> Option<&'static str> {
    let value = integer(bag, id)?;
    let name = usize::try_from(value).ok().and_then(|i| names.get(i));
    if name.is_none() {
        notes.push(format!(
            "property 0x{id:04X} has unknown value {value}; left out"
        ));
    }
    name.copied()
}

/// A PT_SYSTIME property in ISO 8601 UTC, to the second.
fn time(bag: &Bag, id: u16, notes: &mut Vec<String>) -> Option<String> {
    let Some(Value::Time(ticks)) = bag.get(&id) else {
        return None;
    };
    let iso = iso8601(*ticks);
    if iso.is_none() {
        notes.push(format!("property 0x{id:04X} is out of range; left out"));
    }
    iso
}

/// Seconds between 1601-01-01, the FILETIME epoch, and 1970-01-01.
const FILETIME_TO_UNIX: i64 = 11_644_473_600;

/// A FILETIME as "YYYY-MM-DDTHH:MM:SSZ", or None past year 9999.
fn iso8601(ticks: i64) -> Option<String> {
    let seconds = ticks.div_euclid(10_000_000) - FILETIME_TO_UNIX;
    let (days, second_of_day) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    let (year, month, day) = civil_from_days(days);
    (year <= 9999).then(|| {
        format!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
            second_of_day / 3600,
            second_of_day / 60 % 60,
            second_of_day % 60
        )
    })
}

/// The proleptic Gregorian date of a day count from 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetimes_format_as_iso8601_utc() {
        assert_eq!(
            iso8601(130_378_746_040_000_000).as_deref(),
            Some("2014-02-26T07:50:04Z")
        );
        assert_eq!(iso8601(0).as_deref(), Some("1601-01-01T00:00:00Z"));
        assert_eq!(
            iso8601(116_444_736_000_000_000 + 951_782_400 * 10_000_000).as_deref(),
            Some("2000-02-29T00:00:00Z")
        );
        assert_eq!(iso8601(i64::MAX), None);
    }

    #[test]
    fn a_file_name_keeps_only_a_short_alphanumeric_extension() {
        let sha = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(file_name(b"", Some(".png")), format!("{sha}.png"));
        assert_eq!(file_name(b"", Some("JPG")), format!("{sha}.JPG"));
        assert_eq!(file_name(b"", None), sha);
        for odd in ["", ".", "tar.gz", "../x", "ext ension", "cöt", "toolongext"] {
            assert_eq!(file_name(b"", Some(odd)), sha, "{odd:?}");
        }
    }

    #[test]
    fn the_extension_is_the_stored_one_else_the_filenames() {
        let unicode =
            |text: &str| Value::Unicode(text.encode_utf16().flat_map(u16::to_le_bytes).collect());
        let mut props = Bag::from([(tag::ATTACH_LONG_FILENAME, unicode("report.csv"))]);
        let (record, _) = attachment(&props, Some(b""), 1252);
        assert!(record.file.unwrap().ends_with(".csv"));
        props.insert(tag::ATTACH_EXTENSION, unicode(".txt"));
        let (record, _) = attachment(&props, Some(b""), 1252);
        assert!(record.file.unwrap().ends_with(".txt"));
        let (record, _) = attachment(&props, None, 1252);
        assert_eq!((record.file, record.size), (None, None));
    }

    #[test]
    fn an_unknown_attach_method_is_left_out_with_a_note() {
        let props = Bag::from([(tag::ATTACH_METHOD, Value::Integer(3))]);
        let (record, notes) = attachment(&props, None, 1252);
        assert_eq!(record.method, None);
        assert_eq!(notes, ["property 0x3705 has unknown value 3; left out"]);
    }

    #[test]
    fn the_subject_prefix_marker_is_dropped() {
        assert_eq!(strip_subject_prefix("\u{1}\u{5}Re: Hi".into()), "Re: Hi");
        assert_eq!(strip_subject_prefix("Hi".into()), "Hi");
    }
}
