//! The mapping from a MIME message, as mail-parser parses it, to the message model,
//! shared by the .eml reader and the mbox reader. Like the MAPI mapping, it returns its notes for
//! the caller to warn about with the record's place.
//!
//! Each field is the header's: the sender is the first From mailbox, the recipients To's, Cc's
//! and Bcc's mailboxes, groups flattened, each with address type SMTP, and the subject decoded
//! from its encoded words. The submit time is the Date header, and the delivery time the date of
//! the first Received header, the newest hop's, each in UTC. Message-ID, In-Reply-To and
//! References are kept as stored, unfolded, and the transport headers are the raw header block.
//! A MIME message has no message class, so none is written. Where a header appears more than
//! once, the last is read, as mail-parser's own getters read it.

use std::borrow::Cow;

use mail_parser::{AddressList, DateTime, Header, HeaderName, MessageRef};

use crate::mapping;
use crate::model::{Address, Message};

/// The address type of every address a MIME message carries.
const SMTP: &str = "SMTP";

/// Maps one parsed message to a message line.
pub fn message(parsed: &MessageRef<'_>) -> (Message, Vec<String>) {
    let mut notes = Vec::new();
    let headers = parsed.headers();
    let first_received = headers.all(HeaderName::Received).next();
    let message = Message {
        line: "message",
        subject: parsed.subject().map(str::to_owned),
        sender: parsed
            .from()
            .and_then(|from| from.first())
            .and_then(|mailbox| address(mailbox.name(), mailbox.address(), None)),
        to: addresses(parsed.to()),
        cc: addresses(parsed.cc()),
        bcc: addresses(parsed.bcc()),
        submit_time: time(
            headers.get(HeaderName::Date),
            |header| header.value().as_datetime(),
            &mut notes,
        ),
        delivery_time: time(
            first_received,
            |header| header.value().as_received().and_then(|r| r.date()),
            &mut notes,
        ),
        transport_headers: header_block(parsed.root_part().raw_headers(), &mut notes),
        message_id: stored(headers.get(HeaderName::MessageId), &mut notes),
        in_reply_to: stored(headers.get(HeaderName::InReplyTo), &mut notes),
        references: stored(headers.get(HeaderName::References), &mut notes),
        ..Message::default()
    };
    (message, notes)
}

/// An address of its name and address, either of which may be missing but not both.
fn address(name: Option<&str>, address: Option<&str>, kind: Option<&str>) -> Option<Address> {
    let owned = |text: Option<&str>| text.filter(|text| !text.is_empty()).map(str::to_owned);
    let made = Address {
        name: owned(name),
        address: owned(address),
        address_type: kind.map(str::to_owned),
    };
    (made.name.is_some() || made.address.is_some()).then_some(made)
}

/// A recipient header's mailboxes in order, a group's members in its place.
fn addresses(list: Option<AddressList<'_>>) -> Vec<Address> {
    list.into_iter()
        .flat_map(|list| list.mailboxes())
        .filter_map(|mailbox| address(mailbox.name(), mailbox.address(), Some(SMTP)))
        .collect()
}

/// A date header's time in UTC, to the second. One that is present but has no date that reads,
/// or one outside mail-parser's valid range, is left out with a note.
fn time(
    header: Option<Header<'_>>,
    date: impl FnOnce(&Header<'_>) -> Option<DateTime>,
    notes: &mut Vec<String>,
) -> Option<String> {
    let header = header?;
    let utc = date(&header)
        .filter(DateTime::is_valid)
        .and_then(|date| mapping::utc(date.to_timestamp()));
    if utc.is_none() {
        notes.push(format!(
            "the {} header has no date that can be read; left out",
            header.raw_name()
        ));
    }
    utc
}

/// A header's value as stored: its raw bytes, unfolded and trimmed. A value that is blank is
/// left out, and one that isn't valid UTF-8 is decoded lossily, with a note.
fn stored(header: Option<Header<'_>>, notes: &mut Vec<String>) -> Option<String> {
    let header = header?;
    let what = format!("{} header", header.raw_name());
    let raw = lossy(header.raw_value(), &what, notes);
    let unfolded: String = raw.chars().filter(|&c| c != '\r' && c != '\n').collect();
    let trimmed = unfolded.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// The raw header block, without the blank line that ends it, read as [`lossy`] reads it.
fn header_block(raw: &[u8], notes: &mut Vec<String>) -> Option<String> {
    let block = raw
        .strip_suffix(b"\r\n")
        .or_else(|| raw.strip_suffix(b"\n"))
        .filter(|block| block.ends_with(b"\n"))
        .unwrap_or(raw);
    if block.is_empty() {
        return None;
    }
    Some(lossy(block, "header block", notes))
}

/// Raw header bytes as UTF-8, decoded lossily, with a note naming `what`, when they aren't.
fn lossy(raw: &[u8], what: &str, notes: &mut Vec<String>) -> String {
    let text = String::from_utf8_lossy(raw);
    if matches!(text, Cow::Owned(_)) {
        notes.push(format!("the {what} is not valid UTF-8; decoded lossily"));
    }
    text.into_owned()
}
