//! The .pst reader: outlook-pst opens the store, and the walk writes the store line, then each
//! folder depth-first from the IPM subtree's root, its messages in contents-table order before
//! its subfolders.
//!
//! Every record is read whole inside [`guarded`] before its line is written, so a folder or
//! message that fails, or panics, inside outlook-pst becomes a skipped line and a warning, and
//! the walk goes on. Known outlook-pst 1.2.0 faults this has to absorb:
//! - a message node without a sub-node tree fails ("Missing Sub-Node Tree on message"), as the
//!   contacts of java-libpst's dist-list.pst and passworded.pst do;
//! - opening an embedded-message attachment (method 5) locks a std Mutex the reader already
//!   holds and deadlocks, so no attachment is ever opened by its method alone. This slice opens
//!   no attachments.

use std::fs::File;
use std::io::{self, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::rc::Rc;

use outlook_pst::ltp::prop_context::PropertyValue;
use outlook_pst::ltp::table_context::TableContext;
use outlook_pst::messaging::message::Message as PstMessage;
use outlook_pst::messaging::store::{AnsiStore, EntryId, Store, UnicodeStore};
use outlook_pst::ndb::node_id::NodeId;
use outlook_pst::{AnsiPstFile, UnicodePstFile};

use crate::codepage::{message_code_page, store_code_page};
use crate::jsonl::Lines;
use crate::props::{Bag, Value, integer, tag};
use crate::sniff::Encoding;
use crate::{Fault, mapping, model, panic_text, warn};

/// Reads the store at `path`, whose header names `encoding`, and writes its lines to `out`.
pub fn read(path: &Path, encoding: Encoding, out: &mut Lines<impl Write>) -> Result<(), Fault> {
    let refuse = |what: &'static str| move |reason| Fault::Refused(format!("{what}: {reason}"));
    let file =
        File::open(path).map_err(|err| Fault::Refused(format!("{}: {err}", path.display())))?;
    let store: Rc<dyn Store> = match encoding {
        Encoding::Unicode => {
            let pst = guarded(|| UnicodePstFile::read_from(Box::new(file)))
                .map_err(refuse("the PST header can't be read"))?;
            guarded(|| UnicodeStore::read(Rc::new(pst)))
                .map_err(refuse("the store's root can't be read"))?
        }
        Encoding::Ansi => {
            let pst = guarded(|| AnsiPstFile::read_from(Box::new(file)))
                .map_err(refuse("the PST header can't be read"))?;
            guarded(|| AnsiStore::read(Rc::new(pst)))
                .map_err(refuse("the store's root can't be read"))?
        }
    };
    let root = guarded(|| {
        let entry_id = store.properties().ipm_sub_tree_entry_id()?;
        read_folder(&store, &entry_id)
    })
    .map_err(refuse("the IPM subtree can't be read"))?;

    let props = bag(store.properties().iter());
    let code_page = store_code_page(&props);
    let (display_name, notes) = mapping::store_name(&props, code_page);
    warn_all("store", notes);
    out.write(&model::Store {
        line: "store",
        schema: model::SCHEMA,
        format: "pst",
        encoding: Some(encoding.name()),
        password: Some(integer(&props, tag::PST_PASSWORD).is_some_and(|crc| crc != 0)),
        display_name,
    })
    .map_err(Fault::Output)?;

    let mut walk = Walk {
        store,
        code_page,
        out,
    };
    walk.folder(root, None, Vec::new())
}

/// A folder read whole: its properties and the node ids of its messages and subfolders.
struct FolderData {
    id: u32,
    props: Bag,
    messages: Vec<u32>,
    subfolders: Vec<u32>,
}

fn read_folder(store: &Rc<dyn Store>, entry_id: &EntryId) -> io::Result<FolderData> {
    let folder = store.open_folder(entry_id)?;
    let rows = |table: Option<&Rc<dyn TableContext>>| {
        table
            .map(|table| table.rows_matrix().map(|row| u32::from(row.id())).collect())
            .unwrap_or_default()
    };
    Ok(FolderData {
        id: u32::from(folder.properties().node_id()),
        props: bag(folder.properties().iter()),
        messages: rows(folder.contents_table()),
        subfolders: rows(folder.hierarchy_table()),
    })
}

/// A message read whole: its properties and its recipient table's rows.
fn read_message(store: &Rc<dyn Store>, id: u32) -> io::Result<(Bag, Vec<Bag>)> {
    let entry_id = store.properties().make_entry_id(NodeId::from(id))?;
    let message = store.open_message(&entry_id, None)?;
    let props = bag(message.properties().iter());
    Ok((props, recipients(&*message)?))
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

struct Walk<'a, W: Write> {
    store: Rc<dyn Store>,
    code_page: u16,
    out: &'a mut Lines<W>,
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

        for id in data.messages {
            self.message(data.id, id, &place)?;
        }
        for id in data.subfolders {
            let read = guarded(|| {
                let entry_id = self.store.properties().make_entry_id(NodeId::from(id))?;
                read_folder(&self.store, &entry_id)
            });
            match read {
                Ok(sub) => self.folder(sub, Some(data.id), path.clone())?,
                Err(reason) => self.skip("folder", data.id, id, &place, reason)?,
            }
        }
        Ok(())
    }

    fn message(&mut self, folder: u32, id: u32, place: &str) -> Result<(), Fault> {
        let (props, recipients) = match guarded(|| read_message(&self.store, id)) {
            Ok(read) => read,
            Err(reason) => return self.skip("message", folder, id, place, reason),
        };
        let code_page = message_code_page(&props, self.code_page);
        let (mut message, notes) = mapping::message(&props, &recipients, code_page);
        message.id = Some(id);
        message.folder = Some(folder);
        warn_all(&format!("{place}, message {id}"), notes);
        self.out.write(&message).map_err(Fault::Output)
    }

    fn skip(
        &mut self,
        kind: &'static str,
        folder: u32,
        id: u32,
        place: &str,
        reason: String,
    ) -> Result<(), Fault> {
        warn(
            &format!("{place}, {kind} {id}"),
            &format!("skipped: {reason}"),
        );
        self.out
            .write(&model::Skipped {
                line: "skipped",
                kind,
                folder: Some(folder),
                id: Some(id),
                reason,
            })
            .map_err(Fault::Output)
    }
}

/// Runs one read inside outlook-pst, turning its error or its panic into a reason.
fn guarded<T>(read: impl FnOnce() -> io::Result<T>) -> Result<T, String> {
    match catch_unwind(AssertUnwindSafe(read)) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => Err(err.to_string()),
        Err(payload) => Err(format!("the reader panicked: {}", panic_text(&*payload))),
    }
}

fn warn_all(place: &str, notes: Vec<String>) {
    for note in notes {
        warn(place, &note);
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
        PropertyValue::Unicode(value) => Value::Unicode(
            value
                .buffer()
                .iter()
                .flat_map(|unit| unit.to_le_bytes())
                .collect(),
        ),
        _ => Value::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reader_panic_becomes_a_reason() {
        let reason = guarded::<()>(|| panic!("index out of bounds"));
        assert_eq!(
            reason.unwrap_err(),
            "the reader panicked: index out of bounds"
        );
    }

    #[test]
    fn a_reader_error_becomes_a_reason() {
        let reason = guarded::<()>(|| Err(io::Error::other("Missing Sub-Node Tree on message")));
        assert_eq!(reason.unwrap_err(), "Missing Sub-Node Tree on message");
    }
}
