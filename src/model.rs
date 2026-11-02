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

/// One item, whatever its message class.
#[derive(Default, Serialize)]
pub struct Message {
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

/// A message's bodies.
#[derive(Default, Serialize)]
pub struct Body {
    /// The stored plain-text body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl Body {
    pub fn is_empty(&self) -> bool {
        self.text.is_none()
    }
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
    /// The record's own node id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u32>,
    pub reason: String,
}
