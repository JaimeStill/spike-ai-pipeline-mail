//! Test-built .msg files: a message's storages and property streams written through cfb, as
//! MS-OXMSG lays them out, so a test states exactly what a message holds, faults included.

use std::io::Write;
use std::path::{Path, PathBuf};

use cfb::CompoundFile;

/// One property's value, as a .msg stores it.
pub enum Prop {
    Long(i32),
    Unicode(String),
    /// PT_STRING8, in whatever code page the test means.
    String8(Vec<u8>),
    Binary(Vec<u8>),
}

/// A message: its properties, recipients and attachments.
#[derive(Default)]
pub struct Message {
    pub props: Vec<(u16, Prop)>,
    pub recipients: Vec<Child>,
    pub attachments: Vec<Attachment>,
}

/// A recipient's or an attachment's own properties, or a property stream given as raw bytes.
pub enum Child {
    Props(Vec<(u16, Prop)>),
    Raw(Vec<u8>),
}

/// An attachment: its properties, and in its PidTagAttachDataObject storage, an embedded message
/// or the streams of an afStorage storage.
pub struct Attachment {
    pub props: Child,
    pub object: Option<Object>,
}

pub enum Object {
    Message(Message),
    /// An embedded message whose property stream is these bytes.
    Raw(Vec<u8>),
    /// An afStorage storage: its streams, by name.
    Storage(Vec<(&'static str, Vec<u8>)>),
}

/// Writes `message` as a .msg named `name` under Cargo's temporary directory for integration
/// tests.
pub fn build(name: &str, message: &Message) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let mut file = cfb::create(&path).unwrap();
    write_message(&mut file, Path::new("/"), message, 32);
    file.flush().unwrap();
    path
}

const OBJECT: &str = "__substg1.0_3701000D";

fn write_message<F: std::io::Read + Write + std::io::Seek>(
    file: &mut CompoundFile<F>,
    storage: &Path,
    message: &Message,
    header: usize,
) {
    write_props(file, storage, &message.props, header);
    for (number, recipient) in message.recipients.iter().enumerate() {
        let path = storage.join(format!("__recip_version1.0_#{number:08X}"));
        file.create_storage(&path).unwrap();
        write_child(file, &path, recipient);
    }
    for (number, attachment) in message.attachments.iter().enumerate() {
        let path = storage.join(format!("__attach_version1.0_#{number:08X}"));
        file.create_storage(&path).unwrap();
        write_child(file, &path, &attachment.props);
        let object = path.join(OBJECT);
        match &attachment.object {
            None => {}
            Some(Object::Message(embedded)) => {
                file.create_storage(&object).unwrap();
                write_message(file, &object, embedded, 24);
            }
            Some(Object::Raw(bytes)) => {
                file.create_storage(&object).unwrap();
                write_stream(file, &object.join("__properties_version1.0"), bytes);
            }
            Some(Object::Storage(streams)) => {
                file.create_storage(&object).unwrap();
                for (name, bytes) in streams {
                    write_stream(file, &object.join(name), bytes);
                }
            }
        }
    }
}

fn write_child<F: std::io::Read + Write + std::io::Seek>(
    file: &mut CompoundFile<F>,
    storage: &Path,
    child: &Child,
) {
    match child {
        Child::Props(props) => write_props(file, storage, props, 8),
        Child::Raw(bytes) => write_stream(file, &storage.join("__properties_version1.0"), bytes),
    }
}

/// A property stream of `header` zero bytes and an entry per property; a variable-size value
/// goes in its own stream.
fn write_props<F: std::io::Read + Write + std::io::Seek>(
    file: &mut CompoundFile<F>,
    storage: &Path,
    props: &[(u16, Prop)],
    header: usize,
) {
    let mut stream = vec![0; header];
    for (id, prop) in props {
        let (kind, value) = match prop {
            Prop::Long(long) => (0x0003, Stored::Inline(long.to_le_bytes())),
            Prop::Unicode(text) => (
                0x001F,
                Stored::Stream(
                    text.encode_utf16()
                        .chain([0])
                        .flat_map(u16::to_le_bytes)
                        .collect(),
                ),
            ),
            Prop::String8(bytes) => (0x001E, Stored::Stream([&bytes[..], b"\0"].concat())),
            Prop::Binary(bytes) => (0x0102, Stored::Stream(bytes.clone())),
        };
        stream.extend_from_slice(&((u32::from(*id) << 16) | kind).to_le_bytes());
        stream.extend_from_slice(&6u32.to_le_bytes());
        match value {
            Stored::Inline(inline) => stream.extend_from_slice(&inline),
            Stored::Stream(bytes) => {
                stream.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                let name = format!("__substg1.0_{id:04X}{kind:04X}");
                write_stream(file, &storage.join(name), &bytes);
            }
        }
        stream.extend_from_slice(&[0; 4]);
    }
    write_stream(file, &storage.join("__properties_version1.0"), &stream);
}

/// Where a value goes: a fixed-size one inline, a variable-size one in its own stream, with its
/// size inline.
enum Stored {
    Inline([u8; 4]),
    Stream(Vec<u8>),
}

fn write_stream<F: std::io::Read + Write + std::io::Seek>(
    file: &mut CompoundFile<F>,
    path: &Path,
    bytes: &[u8],
) {
    file.create_stream(path).unwrap().write_all(bytes).unwrap();
}
