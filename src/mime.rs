//! The mapping from a MIME message, as mail-parser parses it, to the message model,
//! shared by the .eml reader and the mbox reader. Like the MAPI mapping, it returns its notes for
//! the caller to warn about with the record's place.
//!
//! Each field is the header's: the sender is the first From mailbox, the recipients To's, Cc's
//! and Bcc's mailboxes, groups flattened, each address with address type SMTP, and the subject
//! decoded from its encoded words. The submit time is the Date header, and the delivery time the
//! date of the first Received header, the newest hop's, each in UTC. Message-ID, In-Reply-To and
//! References are kept as stored, unfolded, and the transport headers are the raw header block.
//! A MIME message has no message class, so none is written. Where a header appears more than
//! once, the last is read, as mail-parser's own getters read it.
//!
//! The bodies are mail-parser's body lists (RFC 8621's textBody and htmlBody): the text body is
//! the text parts of the one, and the HTML body the HTML parts of the other, each decoded from
//! its transfer encoding and charset and joined in order; the Markdown body is derived as for a
//! .pst or a .msg. The attachments are the message's parts that aren't body text, in order: its
//! attachments, and the images, audio and video the bodies show. A message/rfc822 part is an
//! embedded message, nested under its record to [`DEPTH_MAX`] messages.
//!
//! mail-parser decodes US-ASCII and ISO-8859-1 as windows-1252, as WHATWG labels them, and the
//! five bytes windows-1252 leaves undefined as U+FFFD. Whatever it reports as a decoding problem
//! (a malformed or unknown transfer encoding, an unknown charset, bytes not valid in the charset)
//! gives a note: the text is decoded lossily. It reports none for a header's encoded words.

use std::borrow::Cow;

use mail_parser::{
    AddressList, DateTime, DecodeProblems, Header, HeaderName, MessageParser, MessagePart,
    MessageRef, PartKind, PartRole,
};

use crate::files::Files;
use crate::guard::guarded;
use crate::model::{Address, Attachment, Body, Derived, Message};
use crate::msg::DEPTH_MAX;
use crate::{Fault, mapping, markdown};

/// The address type of every address a MIME message carries.
const SMTP: &str = "SMTP";

/// A message read: its line, its notes, each with its place below the message ("" for the
/// message itself, else ", attachment <n>" and ", message" for each level of nesting), and the
/// attachments that couldn't be read, by number and reason.
pub struct Read {
    pub message: Message,
    pub notes: Vec<(String, String)>,
    pub skipped: Vec<(u32, String)>,
}

/// The parser both readers parse with: mail-parser's own limits, but transfer-encoded
/// message/rfc822 parts parsed to [`DEPTH_MAX`] levels, as unencoded ones are, where its default
/// stops at 3. Each level decodes from its container, which base64 shrinks, so the memory a
/// message takes stays bounded by its size.
pub fn parser() -> MessageParser {
    MessageParser::new().max_encoded_nesting(DEPTH_MAX)
}

/// Maps one parsed message to a message line, its attachments' contents written through
/// `files`.
pub fn message(parsed: &MessageRef<'_>, files: &mut Files) -> Result<Read, Fault> {
    let mut notes = Vec::new();
    let (message, skipped) = read(parsed, 0, "", files, &mut notes)?;
    Ok(Read {
        message,
        notes,
        skipped,
    })
}

/// Maps the message `parsed`, at `depth` and at the place `at`, with its attachments; embedded
/// messages recursively. Returns it with the attachments that couldn't be read.
fn read(
    parsed: &MessageRef<'_>,
    depth: usize,
    at: &str,
    files: &mut Files,
    notes: &mut Vec<(String, String)>,
) -> Result<(Message, Vec<(u32, String)>), Fault> {
    let mut own = Vec::new();
    let mut message = headers(parsed, &mut own);
    message.body = body(parsed, &mut own);
    notes.extend(own.into_iter().map(|note| (at.to_owned(), note)));
    let mut skipped = Vec::new();
    let parts = parsed
        .parts()
        .filter(|part| part.is_attachment() || matches!(part.role(), PartRole::MediaBody));
    for (number, part) in (1u32..).zip(parts) {
        let here = format!("{at}, attachment {number}");
        let Part {
            mut record,
            content,
            nested,
            notes: own,
        } = match guarded(|| Ok(attachment(&part))) {
            Ok(read) => read,
            Err(failure) => {
                notes.push((here, format!("skipped: {}", failure.reason)));
                skipped.push((number, failure.reason));
                continue;
            }
        };
        notes.extend(own.into_iter().map(|note| (here.clone(), note)));
        if let (Some(file), Some(content)) = (&record.file, &content) {
            files.write(file, content)?;
        }
        match nested {
            Some(_) if depth + 1 > DEPTH_MAX => notes.push((
                here,
                format!(
                    "an embedded message nested deeper than {DEPTH_MAX} messages; \
                     listed without it"
                ),
            )),
            // An attachment inside it that can't be read has its warning; the embedded message
            // has no line to give it a skipped line after.
            Some(nested) => {
                let place = format!("{here}, message");
                let (mut inner, _) = read(&nested, depth + 1, &place, files, notes)?;
                inner.line = "";
                record.message = Some(Box::new(inner));
            }
            None => {}
        }
        message.attachments.push(record);
    }
    Ok((message, skipped))
}

/// The header fields of a message line.
fn headers(parsed: &MessageRef<'_>, notes: &mut Vec<String>) -> Message {
    let headers = parsed.headers();
    let first_received = headers.all(HeaderName::Received).next();
    Message {
        line: "message",
        subject: parsed.subject().map(str::to_owned),
        sender: parsed
            .from()
            .and_then(|from| from.first())
            .and_then(|mailbox| address(mailbox.name(), mailbox.address(), Some(SMTP))),
        to: addresses(parsed.to()),
        cc: addresses(parsed.cc()),
        bcc: addresses(parsed.bcc()),
        submit_time: time(
            headers.get(HeaderName::Date),
            |header| header.value().as_datetime(),
            notes,
        ),
        delivery_time: time(
            first_received,
            |header| header.value().as_received().and_then(|r| r.date()),
            notes,
        ),
        transport_headers: header_block(parsed.root_part().raw_headers(), notes),
        message_id: stored(headers.get(HeaderName::MessageId), notes),
        in_reply_to: stored(headers.get(HeaderName::InReplyTo), notes),
        references: stored(headers.get(HeaderName::References), notes),
        ..Message::default()
    }
}

/// The text and HTML bodies, and the Markdown derived from them. A body list's parts of the
/// other kind, the stand-ins RFC 8621 copies in for a missing version, and its media are no
/// part of it. Parts are joined by a line break, so that one's last line never runs into the
/// next one's first.
fn body(parsed: &MessageRef<'_>, notes: &mut Vec<String>) -> Body {
    let text = joined(
        parsed.text_body(),
        |kind| matches!(kind, PartKind::Text),
        "text body",
        notes,
    );
    let html = joined(
        parsed.html_body(),
        |kind| matches!(kind, PartKind::Html),
        "HTML body",
        notes,
    );
    let markdown = markdown::derive(html.as_deref(), text.as_deref(), notes);
    Body {
        text,
        html,
        rtf: None,
        derived: Derived {
            html: None,
            markdown,
        },
    }
}

/// The text of a body list's parts of one kind, decoded and joined in order, with one note for
/// all of their decoding problems.
fn joined<'m>(
    list: impl Iterator<Item = MessagePart<'m>>,
    kind: impl Fn(&PartKind<'m>) -> bool,
    what: &str,
    notes: &mut Vec<String>,
) -> Option<String> {
    let mut problems = DecodeProblems::default();
    let mut texts = Vec::new();
    for part in list.filter(|part| kind(&part.kind())) {
        if let Some((text, found)) = part.text_checked() {
            problems |= found;
            texts.push(text);
        }
    }
    lossy_note(problems, what, notes);
    (!texts.is_empty()).then(|| texts.join("\n"))
}

/// One attachment part read: its record, its decoded content when it is a file, and its nested
/// message when it is a message/rfc822 part, with its notes.
struct Part<'m> {
    record: Attachment,
    content: Option<Cow<'m, [u8]>>,
    nested: Option<MessageRef<'m>>,
    notes: Vec<String>,
}

/// Reads one attachment part.
fn attachment<'m>(part: &MessagePart<'m>) -> Part<'m> {
    let mut notes = Vec::new();
    let owned = |text: Option<&str>| text.filter(|text| !text.is_empty()).map(str::to_owned);
    let filename = owned(part.attachment_name());
    let mime_type = part.content_type().map(|content_type| {
        let ctype = content_type.ctype();
        match content_type.subtype() {
            Some(subtype) => format!("{ctype}/{subtype}"),
            None => ctype.to_owned(),
        }
    });
    let mut record = Attachment {
        filename,
        mime_type: owned(mime_type.as_deref()),
        content_id: owned(part.content_id()),
        inline: part
            .content_disposition()
            .is_some_and(|disposition| disposition.is_inline()),
        ..Attachment::default()
    };
    let embedded =
        part.is_content_type("message", "rfc822") || part.is_content_type("message", "global");
    if let PartKind::Message(nested) = part.kind() {
        record.method = Some("embedded_message");
        return Part {
            record,
            content: None,
            nested: Some(nested),
            notes,
        };
    }
    if embedded {
        record.method = Some("embedded_message");
        notes.push("the embedded message can't be parsed; listed without it".to_owned());
        return Part {
            record,
            content: None,
            nested: None,
            notes,
        };
    }
    let (content, problems) = part.decoded_checked();
    lossy_note(problems, "attachment's content", &mut notes);
    let extension = record
        .filename
        .as_deref()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, extension)| extension);
    record.file = Some(mapping::file_name(&content, extension));
    record.method = Some("by_value");
    record.size = Some(content.len() as u64);
    Part {
        record,
        content: Some(content),
        nested: None,
        notes,
    }
}

/// A note for the decoding problems mail-parser found in `what`, when it found any.
fn lossy_note(problems: DecodeProblems, what: &str, notes: &mut Vec<String>) {
    if problems.is_empty() {
        return;
    }
    let found: Vec<&str> = [
        (
            DecodeProblems::MALFORMED_TRANSFER_ENCODING,
            "has a malformed transfer encoding",
        ),
        (
            DecodeProblems::UNKNOWN_TRANSFER_ENCODING,
            "has an unknown transfer encoding",
        ),
        (DecodeProblems::UNKNOWN_CHARSET, "has an unknown charset"),
        (
            DecodeProblems::MALFORMED_CHARSET,
            "is not valid in its charset",
        ),
    ]
    .into_iter()
    .filter(|(problem, _)| problems.contains(*problem))
    .map(|(_, phrase)| phrase)
    .collect();
    notes.push(format!(
        "the {what} {}; decoded lossily",
        found.join(" and ")
    ));
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
