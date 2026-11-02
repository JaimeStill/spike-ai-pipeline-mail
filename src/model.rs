//! The lines of schema "1". Each line is one JSON object whose "line" field names it: a store
//! line first, then folder, message and skipped lines. Fields serialize in declaration order,
//! and a property with no value is left out, never written as null.

use serde::Serialize;

/// The schema every line of this output follows.
pub const SCHEMA: &str = "1";

/// The first line: the source's facts.
#[derive(Serialize)]
pub struct Store {
    pub line: &'static str,
    pub schema: &'static str,
    /// "pst" or "msg".
    pub format: &'static str,
    /// For a .pst, "unicode" or "ansi".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<&'static str>,
    /// For a .pst, whether the store is password-protected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// One folder of a .pst, written before its messages and its subfolders.
#[derive(Serialize)]
pub struct Folder {
    pub line: &'static str,
    /// The folder's node id.
    pub id: u32,
    /// The parent folder's node id; the IPM subtree's root has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<u32>,
    /// The display names from the IPM subtree's root down to this folder.
    pub path: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_class: Option<String>,
}

/// One item, whatever its message class: a message line, or a message embedded in another,
/// nested under its attachment record.
#[derive(Default, Serialize)]
pub struct Message {
    /// "message" on a message line. A nested message is no line: its is empty and left out.
    #[serde(skip_serializing_if = "str::is_empty")]
    pub line: &'static str,
    /// In a .pst, the message's node id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u32>,
    /// In a .pst, the node id of the folder the message is in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender: Option<Address>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub to: Vec<Address>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cc: Vec<Address>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bcc: Vec<Address>,
    /// ISO 8601 UTC, to the second, as are the other times.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submit_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modification_time: Option<String>,
    /// "low", "normal" or "high".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub importance: Option<&'static str>,
    /// "normal", "personal", "private" or "confidential".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sensitivity: Option<&'static str>,
    /// The raw transport headers, as stored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport_headers: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_reply_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub references: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_topic: Option<String>,
    /// Lowercase hex of the stored bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_index: Option<String>,
    #[serde(skip_serializing_if = "Body::is_empty")]
    pub body: Body,
    /// In the attachment table's order, less those that couldn't be read.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
}

/// A sender or a recipient.
#[derive(Default, Serialize)]
pub struct Address {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address_type: Option<String>,
}

impl Address {
    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.address.is_none() && self.address_type.is_none()
    }
}

/// A message's bodies. The stored ones sit at the top level, each written as stored; the ones
/// the CLI makes from them sit under `derived`, so a stored body and a derived one never share a
/// place and a reader tells them apart by where they are.
#[derive(Default, Serialize)]
pub struct Body {
    /// The stored plain-text body, PidTagBody.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The stored HTML body, PidTagHtml, decoded by its internet code page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    /// The stored RTF body, PidTagRtfCompressed, decompressed. RTF is 7-bit text; any byte
    /// past ASCII is written as the character of the same number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rtf: Option<String>,
    #[serde(skip_serializing_if = "Derived::is_empty")]
    pub derived: Derived,
}

impl Body {
    pub fn is_empty(&self) -> bool {
        self.text.is_none() && self.html.is_none() && self.rtf.is_none() && self.derived.is_empty()
    }
}

/// The bodies the CLI makes; none of them is stored in the message.
#[derive(Default, Serialize)]
pub struct Derived {
    /// The HTML recovered from the RTF body when it encapsulates HTML (\fromhtml1, MS-OXRTFEX).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    /// Markdown made from the stored HTML, else from the HTML recovered from RTF, else from the
    /// plain text, read literally. Images keep their `cid:` sources, which name attachments'
    /// content_id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,
}

impl Derived {
    pub fn is_empty(&self) -> bool {
        self.html.is_none() && self.markdown.is_none()
    }
}

/// One attachment of a message.
#[derive(Default, Serialize)]
pub struct Attachment {
    /// The name of its content's file: the lowercase hex SHA-256 of the bytes, plus the stored
    /// extension when it is short and alphanumeric. An attachment whose content isn't a file (by
    /// reference, or an embedded message, which a .msg nests under `message`) has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// The long filename, else the short one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// As stored, without angle brackets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_id: Option<String>,
    /// Whether the HTML body refers to it, which is how an inline disposition is stored.
    pub inline: bool,
    /// "none", "by_value", "by_reference", "by_reference_only", "embedded_message", "storage"
    /// or "by_web_reference".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<&'static str>,
    /// The content's size in bytes, when its content is read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// In a .msg, an embedded message's own message, with its bodies and its own attachments,
    /// nested to any depth the reader allows. A .pst's embedded messages have none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Box<Message>>,
}

/// A record that couldn't be read, in place of its line.
#[derive(Serialize)]
pub struct Skipped {
    pub line: &'static str,
    /// "folder", "message" or "attachment".
    pub kind: &'static str,
    /// The node id of the folder the record is in; a skipped folder names its parent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<u32>,
    /// For an attachment, the node id of its message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<u32>,
    /// In a .pst, the record's own node id; an attachment's is its node id within its
    /// message. In a .msg, an attachment's number, the one its storage's name ends in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u32>,
    pub reason: String,
}
