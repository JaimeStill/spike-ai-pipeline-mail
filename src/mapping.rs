//! The one mapping from MAPI properties to the message model, shared by the .pst and .msg
//! readers. It reads only what a reader has put in the bags, and returns its notes (lossy
//! decodings, values it couldn't use) for the caller to warn about with the record's place.

use crate::codepage::Strings;
use crate::model::{Address, Body, Message};
use crate::props::{Bag, Value, integer, tag};

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
        body: Body {
            text: strings.text(props, tag::BODY),
        },
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
    fn the_subject_prefix_marker_is_dropped() {
        assert_eq!(strip_subject_prefix("\u{1}\u{5}Re: Hi".into()), "Re: Hi");
        assert_eq!(strip_subject_prefix("Hi".into()), "Hi");
    }
}
