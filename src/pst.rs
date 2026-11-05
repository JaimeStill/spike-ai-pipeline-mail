//! The .pst reader: outlook-pst opens the store, and the walk writes the store line, then each
//! folder depth-first from the IPM subtree's root, its messages in contents-table order before
//! its subfolders, each message's attachments in its attachment table's order.
//!
//! Every record is read whole inside [`guarded`] before its line is written, so a folder,
//! message or attachment that fails, or panics, inside outlook-pst becomes a skipped line and a
//! warning, and the walk goes on. Known outlook-pst 1.2.0 faults this has to absorb:
//! - a message node without a sub-node tree fails ("Missing Sub-Node Tree on message"), as the
//!   contacts of java-libpst's dist-list.pst and passworded.pst do;
//! - opening an embedded-message attachment (method 5) locks a std Mutex the reader already
//!   holds and deadlocks, and real stores' embedded messages, as in dist-list.pst and Tika's
//!   testPST.pst, omit PidTagAttachMethod on the attachment object, so opening them fails. The
//!   attach method is read from the attachment table instead, and an attachment whose row says
//!   method 5, or gives no method, is never opened: it is listed from its row, without
//!   content, with a warning;
//! - a panic while outlook-pst holds its file's mutex poisons the mutex, and every later read
//!   fails with "Failed to lock PST file". After a panic the walk reopens the store, so the
//!   records after it are still read.
//!
//! With `--after <id>` the walk resumes after the message whose node id is `id`: every folder
//! line, and every skipped line of a folder or a folder's table, is still written, since a
//! consumer places messages by their folders, but a message's line, and the skipped lines of
//! the message and its attachments, are written only for the messages after the cursor's, in
//! the whole read's order. A message at or before the cursor is passed over by its node id in
//! its folder's contents table and never opened, so none of its properties, recipients,
//! attachments or warnings are read: [`Walk::message`] returns before its read. Before any
//! output, the folders are walked once for the cursor's node id in their contents tables, and an
//! id no folder holds is refused.

use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use outlook_pst::ltp::prop_context::PropertyValue;
use outlook_pst::ltp::table_context::{TableContext, TableRowId};
use outlook_pst::messaging::attachment::{
    AnsiAttachment, Attachment, AttachmentData, UnicodeAttachment,
};
use outlook_pst::messaging::message::{AnsiMessage, Message as PstMessage, UnicodeMessage};
use outlook_pst::messaging::store::{AnsiStore, EntryId, Store, UnicodeStore};
use outlook_pst::ndb::node_id::NodeId;
use outlook_pst::{AnsiPstFile, UnicodePstFile};

use crate::codepage::{message_code_page, store_code_page};
use crate::files::Files;
use crate::guard::{Failure, guarded};
use crate::jsonl::Lines;
use crate::props::{Bag, Value, integer, tag};
use crate::sniff::Encoding;
use crate::{Fault, mapping, model, warn, warn_all};

/// PidTagAttachMethod's afEmbeddedMessage (MS-OXCMSG 2.2.2.9).
const AF_EMBEDDED_MESSAGE: i64 = 5;

/// Reads the store at `path`, whose header names `encoding`, writes its lines to `out`, those of
/// its messages after the message with node id `after` when it is given, and its attachments'
/// contents through `files`.
pub fn read(
    path: &Path,
    encoding: Encoding,
    after: Option<u32>,
    out: &mut Lines<impl Write>,
    files: &mut Files,
) -> Result<(), Fault> {
    let pst = Pst::open(path, encoding).map_err(Fault::Refused)?;
    let root = guarded(|| {
        let entry_id = pst.store().properties().ipm_sub_tree_entry_id()?;
        read_folder(pst.store(), &entry_id)
    })
    .map_err(|failure| {
        Fault::Refused(format!("the IPM subtree can't be read: {}", failure.reason))
    })?;
    let props = bag(pst.store().properties().iter());
    let code_page = store_code_page(&props);
    let mut walk = Walk {
        path: path.to_path_buf(),
        encoding,
        pst,
        reopened: 0,
        code_page,
        after,
        out,
        files,
    };
    if let Some(after) = after
        && !walk.holds(&root, after)
    {
        return Err(Fault::Refused(format!(
            "{}: --after {after}: no message in the store has that node id",
            path.display()
        )));
    }
    walk.files.create()?;

    let (display_name, notes) = mapping::store_name(&props, code_page);
    warn_all("store", notes);
    walk.out
        .write(&model::Store {
            line: "store",
            schema: model::SCHEMA,
            format: "pst",
            encoding: Some(encoding.name()),
            password: Some(integer(&props, tag::PST_PASSWORD).is_some_and(|crc| crc != 0)),
            display_name,
        })
        .map_err(Fault::Output)?;
    walk.folder(root, None, Vec::new())
}

/// The store, by its file's encoding: outlook-pst opens an attachment only through the
/// concrete message type, which only the concrete store opens.
enum Pst {
    Unicode(Rc<UnicodeStore>),
    Ansi(Rc<AnsiStore>),
}

impl Pst {
    /// Opens the file at `path` and reads its header and its store's root; a failure is the
    /// refusal's reason.
    fn open(path: &Path, encoding: Encoding) -> Result<Self, String> {
        let file = File::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
        let failed = |what: &str| {
            let what = what.to_owned();
            move |failure: Failure| format!("{what}: {}", failure.reason)
        };
        let header = "the PST header can't be read";
        let root = "the store's root can't be read";
        Ok(match encoding {
            Encoding::Unicode => {
                let pst = guarded(|| UnicodePstFile::read_from(Box::new(file)))
                    .map_err(failed(header))?;
                Self::Unicode(guarded(|| UnicodeStore::read(Rc::new(pst))).map_err(failed(root))?)
            }
            Encoding::Ansi => {
                let pst =
                    guarded(|| AnsiPstFile::read_from(Box::new(file))).map_err(failed(header))?;
                Self::Ansi(guarded(|| AnsiStore::read(Rc::new(pst))).map_err(failed(root))?)
            }
        })
    }

    fn store(&self) -> &dyn Store {
        match self {
            Self::Unicode(store) => &**store,
            Self::Ansi(store) => &**store,
        }
    }

    fn open_message(&self, id: u32) -> io::Result<OpenMessage> {
        let entry_id = self.store().properties().make_entry_id(NodeId::from(id))?;
        Ok(match self {
            Self::Unicode(store) => {
                OpenMessage::Unicode(UnicodeMessage::read(store.clone(), &entry_id, None)?)
            }
            Self::Ansi(store) => {
                OpenMessage::Ansi(AnsiMessage::read(store.clone(), &entry_id, None)?)
            }
        })
    }
}

/// A message open in its store, for its attachments to be opened from.
enum OpenMessage {
    Unicode(Rc<UnicodeMessage>),
    Ansi(Rc<AnsiMessage>),
}

impl OpenMessage {
    fn message(&self) -> &dyn PstMessage {
        match self {
            Self::Unicode(message) => &**message,
            Self::Ansi(message) => &**message,
        }
    }

    fn attachment(&self, id: u32) -> io::Result<Rc<dyn Attachment>> {
        let id = NodeId::from(id);
        Ok(match self {
            Self::Unicode(message) => UnicodeAttachment::read(message.clone(), id, None)?,
            Self::Ansi(message) => AnsiAttachment::read(message.clone(), id, None)?,
        })
    }
}

/// A folder read whole: its properties, the node ids of its messages and subfolders, and the
/// reason each of its tables that went missing can't be read.
struct FolderData {
    id: u32,
    props: Bag,
    messages: Vec<u32>,
    subfolders: Vec<u32>,
    lost_contents: Option<String>,
    lost_hierarchy: Option<String>,
}

fn read_folder(store: &dyn Store, entry_id: &EntryId) -> io::Result<FolderData> {
    let folder = store.open_folder(entry_id)?;
    let props = bag(folder.properties().iter());
    let contents = folder.contents_table();
    let hierarchy = folder.hierarchy_table();
    let lost_contents = (contents.is_none()
        && integer(&props, tag::CONTENT_COUNT).is_some_and(|count| count > 0))
    .then(|| lost("contents", "PidTagContentCount gives it items"));
    let lost_hierarchy = (hierarchy.is_none()
        && matches!(props.get(&tag::SUBFOLDERS), Some(Value::Boolean(true))))
    .then(|| lost("hierarchy", "PidTagSubfolders says it has subfolders"));
    Ok(FolderData {
        id: u32::from(folder.properties().node_id()),
        messages: row_ids(contents),
        subfolders: row_ids(hierarchy),
        props,
        lost_contents,
        lost_hierarchy,
    })
}

/// The reason a folder's table is lost. outlook-pst 1.2.0 gives a table it can't read, or
/// can't find, as no table at all, and its table reader isn't public, so the error itself
/// can't be had: a missing table is told from an empty one only by the folder's own count. A
/// folder whose count is wrong the other way, claiming no items or no subfolders while it has
/// some, still loses them silently.
fn lost(table: &str, count: &str) -> String {
    format!("the {table} table can't be read, though {count} (outlook-pst 1.2.0 gives no reason)")
}

fn row_ids(table: Option<&Rc<dyn TableContext>>) -> Vec<u32> {
    table
        .map(|table| table.rows_matrix().map(|row| u32::from(row.id())).collect())
        .unwrap_or_default()
}

/// A message read whole but for its attachments: the open message, its properties, its
/// recipient table's rows, and its attachments' node ids, in its attachment table's order.
struct MessageData {
    message: OpenMessage,
    props: Bag,
    recipients: Vec<Bag>,
    attachments: Vec<u32>,
}

fn read_message(pst: &Pst, id: u32) -> io::Result<MessageData> {
    let message = pst.open_message(id)?;
    let inner = message.message();
    Ok(MessageData {
        props: bag(inner.properties().iter()),
        recipients: recipients(inner)?,
        attachments: row_ids(inner.attachment_table()),
        message,
    })
}

fn recipients(message: &dyn PstMessage) -> io::Result<Vec<Bag>> {
    let Some(table) = message.recipient_table() else {
        return Ok(Vec::new());
    };
    let context = table.context();
    table
        .rows_matrix()
        .map(|row| {
            let mut bag = Bag::new();
            for (column, value) in context.columns().iter().zip(row.columns(context)?) {
                if let Some(value) = value {
                    let value = table.read_column(&value, column.prop_type())?;
                    bag.insert(column.prop_id(), convert(&value));
                }
            }
            Ok(bag)
        })
        .collect()
}

/// An attachment read whole: its properties, its content when it has some, and whether it was
/// opened or only listed from its attachment table row, with the reader's notes.
struct AttachmentRead {
    props: Bag,
    content: Option<Vec<u8>>,
    opened: bool,
    notes: Vec<String>,
}

/// Reads attachment `id` of `message`. Its attachment table row gives the attach method; an
/// attachment whose row says method 5, or gives none, is never opened (the module's comment
/// says why) and is read from its row alone.
fn read_attachment(message: &OpenMessage, id: u32) -> io::Result<AttachmentRead> {
    let table = message
        .message()
        .attachment_table()
        .ok_or_else(|| io::Error::other("the message has no attachment table"))?;
    let context = table.context();
    let row = table.find_row(TableRowId::new(id))?;
    let mut props = Bag::new();
    let mut notes = Vec::new();
    for (column, value) in context.columns().iter().zip(row.columns(context)?) {
        let Some(value) = value else { continue };
        // A column outlook-pst can't read, as the empty PidTagAttachFilename of libyal's
        // store's rows is, costs only that column.
        match table.read_column(&value, column.prop_type()) {
            Ok(value) => {
                props.insert(column.prop_id(), convert(&value));
            }
            Err(err) => notes.push(format!(
                "attachment table column 0x{:04X} can't be read ({err}); left out",
                column.prop_id()
            )),
        }
    }
    if integer(&props, tag::ATTACH_METHOD).is_none_or(|method| method == AF_EMBEDDED_MESSAGE) {
        return Ok(AttachmentRead {
            props,
            content: None,
            opened: false,
            notes,
        });
    }
    // Opened, the attachment's own properties stand in for its row's, notes and all. Its
    // content is kept apart from them, as stored: PidTagAttachDataBinary for afByValue, the
    // storage's bytes for afStorage; outlook-pst gives none for the by-reference methods.
    let attachment = message.attachment(id)?;
    let props = attachment
        .properties()
        .iter()
        .filter(|(id, _)| **id != tag::ATTACH_DATA)
        .map(|(id, value)| (*id, convert(value)))
        .collect();
    let content = match attachment.data() {
        Some(AttachmentData::Binary(data)) => Some(data.buffer().to_vec()),
        _ => None,
    };
    Ok(AttachmentRead {
        props,
        content,
        opened: true,
        notes: Vec::new(),
    })
}

struct Walk<'a, W: Write> {
    path: PathBuf,
    encoding: Encoding,
    pst: Pst,
    /// How many times the store has been reopened after a panic. A message opened before the
    /// last reopen holds the poisoned store, so it is opened again before its next attachment.
    reopened: u32,
    code_page: u16,
    /// The node id of the message the walk resumes after, until the walk passes it; None once
    /// it has, or for a whole read.
    after: Option<u32>,
    out: &'a mut Lines<W>,
    files: &'a mut Files,
}

impl<W: Write> Walk<'_, W> {
    fn folder(
        &mut self,
        data: FolderData,
        parent: Option<u32>,
        mut path: Vec<String>,
    ) -> Result<(), Fault> {
        let (display_name, container_class, notes) = mapping::folder(&data.props, self.code_page);
        path.push(display_name.clone().unwrap_or_default());
        let place = format!("/{}", path.join("/"));
        warn_all(&place, notes);
        self.out
            .write(&model::Folder {
                line: "folder",
                id: data.id,
                parent,
                path: path.clone(),
                display_name,
                container_class,
            })
            .map_err(Fault::Output)?;

        if let Some(reason) = data.lost_contents {
            warn(
                &format!("{place}, contents table"),
                &format!("skipped: {reason}"),
            );
            self.skipped("contents", data.id, None, None, reason)?;
        }
        for id in data.messages {
            self.message(data.id, id, &place)?;
        }
        if let Some(reason) = data.lost_hierarchy {
            warn(
                &format!("{place}, hierarchy table"),
                &format!("skipped: {reason}"),
            );
            self.skipped("hierarchy", data.id, None, None, reason)?;
        }
        for id in data.subfolders {
            let read = self.read(|pst| {
                let entry_id = pst.store().properties().make_entry_id(NodeId::from(id))?;
                read_folder(pst.store(), &entry_id)
            });
            match read {
                Ok(sub) => self.folder(sub, Some(data.id), path.clone())?,
                Err(reason) => {
                    warn(
                        &format!("{place}, folder {id}"),
                        &format!("skipped: {reason}"),
                    );
                    self.skipped("folder", data.id, None, Some(id), reason)?;
                }
            }
        }
        Ok(())
    }

    /// Whether `data`'s folder or a folder below it holds message `id` in its contents table,
    /// read the way [`Walk::folder`] reads them, depth-first, stopping at the first that does.
    /// A subfolder that can't be read is passed over here and skipped, with its warning, by
    /// the walk itself.
    fn holds(&mut self, data: &FolderData, id: u32) -> bool {
        if data.messages.contains(&id) {
            return true;
        }
        data.subfolders.iter().any(|&sub| {
            let read = self.read(|pst| {
                let entry_id = pst.store().properties().make_entry_id(NodeId::from(sub))?;
                read_folder(pst.store(), &entry_id)
            });
            read.is_ok_and(|sub| self.holds(&sub, id))
        })
    }

    /// Writes one message's line, its attachments' files before it, and after it a skipped line
    /// for each attachment that couldn't be read. Until the walk passes the `--after` cursor's
    /// message, a message is passed over unopened.
    fn message(&mut self, folder: u32, id: u32, place: &str) -> Result<(), Fault> {
        if let Some(after) = self.after {
            if id == after {
                self.after = None;
            }
            return Ok(());
        }
        let place = format!("{place}, message {id}");
        let data = match self.read(|pst| read_message(pst, id)) {
            Ok(data) => data,
            Err(reason) => {
                warn(&place, &format!("skipped: {reason}"));
                return self.skipped("message", folder, None, Some(id), reason);
            }
        };
        let code_page = message_code_page(&data.props, self.code_page);
        let (mut message, notes) = mapping::message(&data.props, &data.recipients, code_page);
        message.id = Some(id);
        message.folder = Some(folder);
        warn_all(&place, notes);

        let mut open = data.message;
        let mut opened_in = self.reopened;
        let mut skipped = Vec::new();
        for attachment in data.attachments {
            let here = format!("{place}, attachment {attachment}");
            let reopened = self.reopened;
            let read = self.read(|pst| {
                if opened_in != reopened {
                    open = pst.open_message(id)?;
                    opened_in = reopened;
                }
                read_attachment(&open, attachment)
            });
            let read = match read {
                Ok(read) => read,
                Err(reason) => {
                    warn(&here, &format!("skipped: {reason}"));
                    skipped.push((attachment, reason));
                    continue;
                }
            };
            warn_all(&here, read.notes);
            if !read.opened {
                warn(&here, not_opened(&read.props));
            }
            let (record, notes) =
                mapping::attachment(&read.props, read.content.as_deref(), code_page);
            warn_all(&here, notes);
            if let (Some(file), Some(content)) = (&record.file, &read.content) {
                self.files.write(file, content)?;
            }
            message.attachments.push(record);
        }
        self.out.write(&message).map_err(Fault::Output)?;
        for (attachment, reason) in skipped {
            self.skipped("attachment", folder, Some(id), Some(attachment), reason)?;
        }
        Ok(())
    }

    /// Runs one read against the store inside [`guarded`]. A panic may have poisoned the
    /// store's file mutex, so after one the store is reopened for the reads that follow.
    fn read<T>(&mut self, read: impl FnOnce(&Pst) -> io::Result<T>) -> Result<T, String> {
        let pst = &self.pst;
        guarded(|| read(pst)).map_err(|failure| {
            if failure.panicked {
                self.reopen();
            }
            failure.reason
        })
    }

    fn reopen(&mut self) {
        match Pst::open(&self.path, self.encoding) {
            Ok(pst) => {
                self.pst = pst;
                self.reopened += 1;
            }
            Err(reason) => warn(
                "store",
                &format!(
                    "can't be reopened after a reader panic ({reason}); \
                     the records after it may be skipped"
                ),
            ),
        }
    }

    /// Writes the skipped line of a record whose warning is already written. A lost table has
    /// no id of its own; its folder is the folder whose table it is.
    fn skipped(
        &mut self,
        kind: &'static str,
        folder: u32,
        message: Option<u32>,
        id: Option<u32>,
        reason: String,
    ) -> Result<(), Fault> {
        self.out
            .write(&model::Skipped {
                line: "skipped",
                kind,
                folder: Some(folder),
                message,
                id,
                reason,
            })
            .map_err(Fault::Output)
    }
}

/// The warning for an attachment listed from its table row, never opened.
fn not_opened(row: &Bag) -> &'static str {
    if integer(row, tag::ATTACH_METHOD) == Some(AF_EMBEDDED_MESSAGE) {
        "an embedded message (attach method 5), listed without its content: \
         outlook-pst 1.2.0 can't open one"
    } else {
        "the attachment table gives no attach method; listed without its content, \
         never opened, as it may be an embedded message"
    }
}

fn bag<'a>(props: impl Iterator<Item = (&'a u16, &'a PropertyValue)>) -> Bag {
    props.map(|(id, value)| (*id, convert(value))).collect()
}

fn convert(value: &PropertyValue) -> Value {
    match value {
        PropertyValue::Integer16(value) => Value::Integer(i64::from(*value)),
        PropertyValue::Integer32(value) => Value::Integer(i64::from(*value)),
        PropertyValue::Integer64(value) => Value::Integer(*value),
        PropertyValue::Boolean(value) => Value::Boolean(*value),
        PropertyValue::Time(value) => Value::Time(*value),
        PropertyValue::Binary(value) => Value::Binary(value.buffer().to_vec()),
        PropertyValue::String8(value) => Value::String8(value.buffer().to_vec()),
        PropertyValue::Unicode(value) => Value::Unicode(utf16le(value.buffer())),
        PropertyValue::MultipleInteger16(values) => {
            multiple(values, |v| Value::Integer((*v).into()))
        }
        PropertyValue::MultipleInteger32(values) => {
            multiple(values, |v| Value::Integer((*v).into()))
        }
        PropertyValue::MultipleInteger64(values) => multiple(values, |v| Value::Integer(*v)),
        PropertyValue::MultipleTime(values) => multiple(values, |v| Value::Time(*v)),
        PropertyValue::MultipleBinary(values) => {
            multiple(values, |v| Value::Binary(v.buffer().to_vec()))
        }
        PropertyValue::MultipleString8(values) => {
            multiple(values, |v| Value::String8(v.buffer().to_vec()))
        }
        PropertyValue::MultipleUnicode(values) => {
            multiple(values, |v| Value::Unicode(utf16le(v.buffer())))
        }
        _ => Value::Other,
    }
}

fn multiple<T>(values: &[T], convert: impl Fn(&T) -> Value) -> Value {
    Value::Multiple(values.iter().map(convert).collect())
}

fn utf16le(units: &[u16]) -> Vec<u8> {
    units.iter().flat_map(|unit| unit.to_le_bytes()).collect()
}
