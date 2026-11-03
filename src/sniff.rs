//! The input's format, from its first bytes and never from its name.

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
    /// Neither a .pst nor a .msg.
    Unknown,
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

pub fn sniff(prefix: &[u8]) -> Format {
    if prefix.starts_with(CFB_MAGIC) {
        return Format::CompoundFile;
    }
    if !prefix.starts_with(PST_MAGIC) || prefix.len() < PREFIX {
        return Format::Unknown;
    }
    let client = &prefix[8..10];
    if client == OST_CLIENT {
        return Format::Ost;
    }
    if client != PST_CLIENT {
        return Format::Unknown;
    }
    // HEADER.wVer: 14 and 15 are ANSI, 23 is Unicode; outlook-pst reads no other.
    match u16::from_le_bytes([prefix[10], prefix[11]]) {
        14 | 15 => Format::Pst(Encoding::Ansi),
        23 => Format::Pst(Encoding::Unicode),
        version => Format::PstVersion(version),
    }
}
