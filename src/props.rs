//! MAPI properties as both readers hand them to the mapping: a bag of property values keyed by
//! property id, in the reader-neutral form of [`Value`]. The .pst reader fills bags from
//! outlook-pst; the .msg reader fills them from the compound file's property streams.

use std::collections::BTreeMap;

/// The property ids the mapping reads, from MS-OXPROPS.
pub mod tag {
    pub const IMPORTANCE: u16 = 0x0017;
    pub const MESSAGE_CLASS: u16 = 0x001A;
    pub const SENSITIVITY: u16 = 0x0036;
    pub const SUBJECT: u16 = 0x0037;
    pub const CLIENT_SUBMIT_TIME: u16 = 0x0039;
    pub const CONVERSATION_TOPIC: u16 = 0x0070;
    pub const CONVERSATION_INDEX: u16 = 0x0071;
    pub const TRANSPORT_MESSAGE_HEADERS: u16 = 0x007D;
    pub const RECIPIENT_TYPE: u16 = 0x0C15;
    pub const SENDER_NAME: u16 = 0x0C1A;
    pub const SENDER_ADDRESS_TYPE: u16 = 0x0C1E;
    pub const SENDER_EMAIL_ADDRESS: u16 = 0x0C1F;
    pub const MESSAGE_DELIVERY_TIME: u16 = 0x0E06;
    pub const BODY: u16 = 0x1000;
    pub const RTF_COMPRESSED: u16 = 0x1009;
    /// PidTagHtml, PT_BINARY; PidTagBodyHtml is the same id as a string.
    pub const HTML: u16 = 0x1013;
    pub const INTERNET_MESSAGE_ID: u16 = 0x1035;
    pub const INTERNET_REFERENCES: u16 = 0x1039;
    pub const IN_REPLY_TO_ID: u16 = 0x1042;
    pub const DISPLAY_NAME: u16 = 0x3001;
    pub const ADDRESS_TYPE: u16 = 0x3002;
    pub const EMAIL_ADDRESS: u16 = 0x3003;
    pub const CREATION_TIME: u16 = 0x3007;
    pub const LAST_MODIFICATION_TIME: u16 = 0x3008;
    pub const CONTAINER_CLASS: u16 = 0x3613;
    pub const ATTACH_DATA: u16 = 0x3701;
    pub const ATTACH_EXTENSION: u16 = 0x3703;
    pub const ATTACH_FILENAME: u16 = 0x3704;
    pub const ATTACH_METHOD: u16 = 0x3705;
    pub const ATTACH_LONG_FILENAME: u16 = 0x3707;
    pub const ATTACH_MIME_TAG: u16 = 0x370E;
    pub const ATTACH_CONTENT_ID: u16 = 0x3712;
    pub const ATTACH_FLAGS: u16 = 0x3714;
    pub const INTERNET_CODEPAGE: u16 = 0x3FDE;
    pub const MESSAGE_CODEPAGE: u16 = 0x3FFD;
    pub const CODE_PAGE_ID: u16 = 0x66C3;
    pub const PST_PASSWORD: u16 = 0x67FF;
}

/// One property's value. Strings keep their stored bytes: decoding them needs the code page,
/// which only the mapping knows.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// PT_SHORT, PT_LONG or PT_I8.
    Integer(i64),
    /// PT_BOOLEAN.
    Boolean(bool),
    /// PT_SYSTIME: 100-nanosecond intervals since 1601-01-01 UTC.
    Time(i64),
    /// PT_BINARY.
    Binary(Vec<u8>),
    /// PT_STRING8: bytes in the code page of the message, else of the store.
    String8(Vec<u8>),
    /// PT_UNICODE: UTF-16LE bytes, without the terminating null.
    Unicode(Vec<u8>),
    /// Any other type; the mapping reads none.
    Other,
}

/// A record's properties by property id. A BTreeMap, so every walk over it is in id order.
pub type Bag = BTreeMap<u16, Value>;

/// The value of an integer property, when it is one.
pub fn integer(bag: &Bag, id: u16) -> Option<i64> {
    match bag.get(&id)? {
        Value::Integer(value) => Some(*value),
        _ => None,
    }
}
