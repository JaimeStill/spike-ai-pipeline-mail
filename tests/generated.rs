//! The generated fixtures under tests/fixtures/generated: the .pst stores and the .msg files
//! built from the hand-written .eml and item sources, read through the binary and checked against
//! the sources' truth, and the manifest checked against the committed files.
//!
//! The truth (truth.json) is the sources'; what each format carries of it is stated here. PST
//! Builder 1.3.0, through its EmlMapper, writes the subject, the sender's name and address, the To
//! and Cc recipients, the times, the plain-text and HTML bodies and the attachments. It writes no
//! Bcc, no sender address type, no threading fields, no transport headers and no conversation
//! index, and it skips message/rfc822 parts, so a generated store has no embedded messages. Its
//! importance and sensitivity are always normal, and its conversation topic is the subject. It
//! writes each attachment by value, with no PidTagAttachExtension, and marks an inline one with
//! ATT_MHTML_REF. It stores the HTML body as UTF-8 with no PidTagInternetCodepage, and no RTF
//! body, so a generated store has no HTML recovered from RTF.
//!
//! MsgKit 3.0.6, through Converter.ConvertEmlToMsg, writes the subject, the sender and every
//! recipient, Bcc included, each with address type SMTP, the importance, the plain-text and HTML
//! bodies, no RTF body, and the attachments by value. Its gaps, asserted as the files hold them:
//! - the submit and delivery times are both the Date header, and the creation and modification
//!   times are the time of conversion;
//! - Message-ID, In-Reply-To and References are stored without their angle brackets, and
//!   References keeps only its last ID;
//! - the conversation topic is the subject less its "Re: " or "Fwd: ", and there is no
//!   sensitivity and no conversation index;
//! - the transport headers are MimeKit's rewriting of the source's;
//! - an attachment's type comes from MsgKit's own table of extensions (MSGKIT_MIME_TYPES);
//! - a message/rfc822 part is attached as a by-value .eml file named by its subject, not as an
//!   embedded message, so a generated .msg has no embedded message either.
//!
//! An item source (items/*.json) is written field by field through PST Builder's MessageItem, for
//! the classes an .eml can't carry: conversations.pst's Conversation History folder holds Skype
//! for Business text chats (IPM.Note.Microsoft.Conversation), a missed IM
//! (IPM.Note.Microsoft.Missed) and an IPM.Note beside them. The truth's items section is the
//! store's own, every field stated: the class, the subject, the sender, the recipients, the four
//! times, the HTML body, the item's only body, and the Markdown the CLI makes from it. PST Builder
//! adds what it adds to an .eml's message: no sender address type, normal importance and
//! sensitivity, and the subject as the conversation topic; the generator gives each recipient the
//! address type SMTP.

mod common;

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use common::{built, fixture, names, run, run_in, scratch, sha256};
use serde_json::{Value, json};

/// The folders PST Builder 1.3.0 makes in every store before the layout's, under the IPM
/// subtree's root, "Top of Personal Folders".
const BUILDER_FOLDERS: &[&str] = &["Deleted Items"];

fn generated(path: &str) -> std::path::PathBuf {
    fixture(&format!("generated/{path}"))
}

fn json_file(path: &str) -> Value {
    serde_json::from_slice(&std::fs::read(generated(path)).unwrap()).unwrap()
}

/// A layout folder's list of sources, empty when it is left out.
fn optional_strings(value: &Value) -> Vec<String> {
    if value.is_null() {
        vec![]
    } else {
        strings(value)
    }
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect()
}

/// Each store's folder paths, under the root, depth-first: a folder's subfolders in the order
/// they were made, which is PST Builder's folders first, then the layout's, each path's parents
/// made before it.
fn expected_folders(store: &Value) -> Vec<Vec<String>> {
    let mut made: Vec<Vec<String>> = vec![vec![]];
    let paths = BUILDER_FOLDERS
        .iter()
        .map(|name| vec![(*name).to_owned()])
        .chain(
            store["folders"]
                .as_array()
                .unwrap()
                .iter()
                .map(|folder| strings(&folder["path"])),
        );
    for path in paths {
        for depth in 1..=path.len() {
            if !made.contains(&path[..depth].to_vec()) {
                made.push(path[..depth].to_vec());
            }
        }
    }
    // Depth-first is the order of each path's parents' creation indexes, then its own.
    let order = |path: &Vec<String>| -> Vec<usize> {
        (1..=path.len())
            .map(|depth| made.iter().position(|p| p[..] == path[..depth]).unwrap())
            .collect()
    };
    let mut folders = made.clone();
    folders.sort_by_key(order);
    folders
}

/// What the store holds of one recipient list: EmlMapper names a recipient without a display
/// name by its address, and gives every recipient the address type SMTP.
fn recipients(truth: Option<&Value>) -> Option<Value> {
    let list = truth?.as_array().unwrap();
    let mapped = list
        .iter()
        .map(|r| {
            json!({
                "name": r.get("name").unwrap_or(&r["address"]),
                "address": r["address"],
                "address_type": "SMTP",
            })
        })
        .collect::<Vec<_>>();
    (!mapped.is_empty()).then_some(Value::Array(mapped))
}

/// What the store holds of a source's attachments, embedded messages left out: each by value,
/// its file named by its SHA-256 and its filename's extension.
fn attachments(truth: &Value) -> Option<Value> {
    let list = truth.get("attachments")?.as_array().unwrap();
    let mapped = list
        .iter()
        .map(|a| {
            let filename = a["filename"].as_str().unwrap();
            let (_, extension) = filename.rsplit_once('.').unwrap();
            let mut record = json!({
                "file": format!("{}.{extension}", a["sha256"].as_str().unwrap()),
                "filename": filename,
                "mime_type": a["mime_type"],
                "inline": a["inline"],
                "method": "by_value",
                "size": a["size"],
            });
            if let Some(cid) = a.get("content_id") {
                record["content_id"] = cid.clone();
            }
            record
        })
        .collect();
    Some(Value::Array(mapped))
}

/// One message line against its source's truth, as the header comment says a store carries it.
fn assert_message(got: &Value, truth: &Value, source: &str) {
    let expect = |field: &str, value: Option<&Value>| {
        assert_eq!(got.get(field), value, "{source}: {field}");
    };
    expect("class", Some(&json!("IPM.Note")));
    expect("subject", truth.get("subject"));
    expect("sender", truth.get("sender"));
    expect("to", recipients(truth.get("to")).as_ref());
    expect("cc", recipients(truth.get("cc")).as_ref());
    expect("bcc", None);
    // The generator passes the Received date as the delivery time; EmlMapper uses it for the
    // creation and modification times too, and the Date header for the submit time.
    let date = &truth["date"];
    let received = truth.get("received").unwrap_or(date);
    expect("submit_time", Some(date));
    expect("delivery_time", Some(received));
    expect("creation_time", Some(received));
    expect("modification_time", Some(received));
    expect("importance", Some(&json!("normal")));
    expect("sensitivity", Some(&json!("normal")));
    expect("conversation_topic", truth.get("subject"));
    for field in [
        "transport_headers",
        "message_id",
        "in_reply_to",
        "references",
        "conversation_index",
    ] {
        expect(field, None);
    }
    expect("attachments", attachments(truth).as_ref());
    for body in ["text", "html"] {
        let got = got["body"]
            .get(body)
            .map(|text| text.as_str().unwrap().replace("\r\n", "\n"));
        assert_eq!(
            got.as_deref(),
            truth["body"].get(body).map(|t| t.as_str().unwrap()),
            "{source}: body {body}"
        );
    }
    assert_eq!(got["body"].get("rtf"), None, "{source}: body rtf");
    let derived = got["body"]["derived"].as_object().unwrap();
    assert_eq!(
        derived.keys().collect::<Vec<_>>(),
        ["markdown"],
        "{source}: derived bodies"
    );
}

/// One item line against its item source's truth, every field the truth states, and nothing the
/// generator doesn't write.
fn assert_item(got: &Value, truth: &Value, source: &str) {
    let expect = |field: &str, value: Option<&Value>| {
        assert_eq!(got.get(field), value, "{source}: {field}");
    };
    for field in [
        "class",
        "subject",
        "sender",
        "submit_time",
        "delivery_time",
        "creation_time",
        "modification_time",
    ] {
        expect(field, Some(&truth[field]));
    }
    expect("to", recipients(truth.get("to")).as_ref());
    expect("cc", recipients(truth.get("cc")).as_ref());
    expect("bcc", None);
    expect("importance", Some(&json!("normal")));
    expect("sensitivity", Some(&json!("normal")));
    expect("conversation_topic", Some(&truth["subject"]));
    for field in [
        "transport_headers",
        "message_id",
        "in_reply_to",
        "references",
        "conversation_index",
        "attachments",
    ] {
        expect(field, None);
    }
    assert_eq!(
        got["body"],
        json!({"html": truth["html"], "derived": {"markdown": truth["markdown"]}}),
        "{source}: body"
    );
}

/// The message line `source` became in mailbox.pst.
fn message_from(source: &str) -> Value {
    let layout = json_file("layout.json");
    let store = &layout["stores"][0];
    let sources: Vec<String> = store["folders"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|folder| strings(&folder["messages"]))
        .collect();
    assert!(sources.iter().any(|s| s == source), "{source}");
    let subject = &json_file("truth.json")["messages"][source]["subject"];
    run(&[generated(store["file"].as_str().unwrap())])
        .lines()
        .into_iter()
        .find(|line| line["line"] == "message" && &line["subject"] == subject)
        .unwrap()
}

fn markdown(message: &Value) -> &str {
    message["body"]["derived"]["markdown"].as_str().unwrap()
}

/// The Markdown body of an HTML message is htmd's rendering of its stored HTML.
#[test]
fn markdown_is_made_from_the_html() {
    let message = message_from("07-html-only-bcc.eml");
    assert_eq!(
        markdown(&message),
        "Shall we meet at the café at noon? ☕\n\n*   Bring Note G\n*   Bring the moth"
    );
}

/// An inline picture keeps its cid: source in the Markdown, and it names the content_id of the
/// attachment it is.
#[test]
fn markdown_keeps_cid_images() {
    let message = message_from("03-html-inline-image.eml");
    assert_eq!(
        markdown(&message),
        "We found a **moth** in relay 70, panel F.\n\n![moth](cid:moth@example.net)"
    );
    assert_eq!(message["attachments"][0]["content_id"], "moth@example.net");
}

/// Without HTML, the Markdown is the plain text, escaped to read literally: the quoted line is
/// no Markdown quote.
#[test]
fn markdown_from_plain_text_reads_literally() {
    let message = message_from("02-reply-latin1.eml");
    assert_eq!(message["body"].get("html"), None);
    assert_eq!(
        markdown(&message),
        "Ada,\n\nSplendid. The café in Zürich serves crème brûlée; we shall  \n\
         discuss Note G there.\n\n\\> I have finished the notes on the engine.\n\nCharles"
    );
}

/// Each generated store, read through the binary, holds the layout's folder tree, and in each
/// folder its .eml sources' messages, then its item sources' items, in order, each matching its
/// source's truth.
#[test]
fn stores_match_the_truth() {
    let layout = json_file("layout.json");
    let truth = json_file("truth.json");
    for store in layout["stores"].as_array().unwrap() {
        let file = store["file"].as_str().unwrap();
        let out = run(&[generated(file)]);
        assert_eq!(out.code, Some(0), "{file}: {}", out.stderr);
        assert_eq!(out.stderr, "", "{file}");
        let lines = out.lines();
        assert_eq!(lines[0]["display_name"], store["display_name"], "{file}");

        let mut paths: HashMap<u64, Vec<String>> = HashMap::new();
        let mut folders = Vec::new();
        let mut messages: HashMap<Vec<String>, Vec<&Value>> = HashMap::new();
        for line in &lines[1..] {
            match line["line"].as_str().unwrap() {
                "folder" => {
                    let path = strings(&line["path"]);
                    assert_eq!(path[0], "Top of Personal Folders", "{file}");
                    let path = path[1..].to_vec();
                    paths.insert(line["id"].as_u64().unwrap(), path.clone());
                    folders.push(path);
                }
                "message" => {
                    let path = paths[&line["folder"].as_u64().unwrap()].clone();
                    messages.entry(path).or_default().push(line);
                }
                other => panic!("{file}: a {other} line"),
            }
        }
        assert_eq!(folders, expected_folders(store), "{file}: folders");

        for folder in store["folders"].as_array().unwrap() {
            let path = strings(&folder["path"]);
            let sources = optional_strings(&folder["messages"]);
            let items = optional_strings(&folder["items"]);
            let got = messages.remove(&path).unwrap_or_default();
            assert_eq!(
                got.len(),
                sources.len() + items.len(),
                "{file}: messages in {path:?}"
            );
            let (from_eml, from_items) = got.split_at(sources.len());
            for (message, source) in from_eml.iter().zip(&sources) {
                assert_message(message, &truth["messages"][source], source);
            }
            for (item, source) in from_items.iter().zip(&items) {
                assert_item(item, &truth["items"][source], source);
            }
        }
        assert!(
            messages.is_empty(),
            "{file}: messages outside the layout's folders"
        );
    }
}

/// The truth's attachments cover what the attachment record has to get right: an inline picture
/// with a content-id, and a filename in non-ASCII, which its source gives in RFC 2231's encoding.
#[test]
fn the_truth_covers_inline_and_non_ascii_attachments() {
    let truth = json_file("truth.json");
    let attachments: Vec<&Value> = truth["messages"]
        .as_object()
        .unwrap()
        .values()
        .filter_map(|message| message.get("attachments")?.as_array())
        .flatten()
        .collect();
    assert!(
        attachments
            .iter()
            .any(|a| a["inline"] == true && a.get("content_id").is_some())
    );
    assert!(
        attachments
            .iter()
            .any(|a| !a["filename"].as_str().unwrap().is_ascii())
    );
    let source = std::fs::read_to_string(generated("eml/04-attachment-cyrillic.eml")).unwrap();
    assert!(source.contains("filename*="), "an RFC 2231 filename");
}

/// The store's records, every attachment's file name across its message lines.
fn files_named(lines: &[Value]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|line| line.get("attachments")?.as_array())
        .flatten()
        .filter_map(|a| Some(a.get("file")?.as_str()?.to_owned()))
        .collect()
}

/// With --attachments, each record's file is in the directory, created as it was missing, its
/// bytes those its name hashes, and a store without attachments leaves it empty; stdout is the
/// same as without the flag.
#[test]
fn attachments_are_written_by_content() {
    let mut written = 0;
    for store in json_file("layout.json")["stores"].as_array().unwrap() {
        let file = generated(store["file"].as_str().unwrap());
        let stem = file.file_stem().unwrap().to_str().unwrap();
        let dir = scratch(&format!("generated-attachments-{stem}")).join("created");
        let out = run(&[file.as_os_str(), "--attachments".as_ref(), dir.as_os_str()]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, run(&[&file]).stdout);
        let mut expected = files_named(&out.lines());
        expected.sort_unstable();
        expected.dedup();
        assert_eq!(names(&dir), expected);
        written += expected.len();
        for name in expected {
            let (sha, _) = name.split_once('.').unwrap();
            assert_eq!(sha256(&dir.join(&name)), sha, "{name}");
        }
    }
    assert!(written > 0, "some store has attachments");
}

/// Without --attachments, a run writes nothing to disk: its working directory stays empty.
#[test]
fn nothing_is_written_without_the_flag() {
    let cwd = scratch("generated-no-flag");
    let out = run_in(&cwd, &[generated("pst/mailbox.pst")]);
    assert_eq!(out.code, Some(0));
    assert!(!files_named(&out.lines()).is_empty());
    assert_eq!(names(&cwd), Vec::<String>::new());
}

/// Content that can't be written exits 1 with one error line: a directory that can't be
/// created, and a file name already taken by a directory, which no user's permissions bypass.
#[test]
fn content_that_cant_be_written_exits_1() {
    let store = generated("pst/mailbox.pst");
    let scratch = scratch("generated-unwritable");
    let file = scratch.join("a-file");
    std::fs::write(&file, b"").unwrap();
    let out = run(&[
        store.as_os_str(),
        "--attachments".as_ref(),
        file.join("dir").as_os_str(),
    ]);
    assert_eq!(out.code, Some(1));
    assert!(
        out.stderr
            .starts_with("spike-ai-pipeline-mail: error: the attachments directory ")
            && out.stderr.contains("can't be created"),
        "{}",
        out.stderr
    );

    let dir = scratch.join("taken");
    let first = files_named(&run(&[&store]).lines()).remove(0);
    std::fs::create_dir_all(dir.join(&first)).unwrap();
    let out = run(&[store.as_os_str(), "--attachments".as_ref(), dir.as_os_str()]);
    assert_eq!(out.code, Some(1));
    assert!(
        out.stderr.starts_with(&format!(
            "spike-ai-pipeline-mail: error: the attachment {}",
            dir.join(&first).display()
        )) && out.stderr.contains("can't be written"),
        "{}",
        out.stderr
    );
    assert_eq!(out.stderr.lines().count(), 1);
}

/// MS-PST's CRC (MS-PST 5.3): CRC-32's reflected polynomial from 0, without a final inversion.
fn pst_crc(bytes: &[u8]) -> u32 {
    let mut crc = 0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    crc
}

/// mailbox.pst with note-g.txt's PidTagAttachMethod rewritten to `method` on the attachment
/// object, and its block's CRC made good. The store is unencrypted: the property's record is in
/// the data block at 28864, 232 bytes long, its value at 29039, the CRC in its trailer at 29108.
/// Its attachment table row keeps method 1, so the CLI opens it.
fn with_attach_method(method: u8) -> std::path::PathBuf {
    let mut bytes = std::fs::read(generated("pst/mailbox.pst")).unwrap();
    let (block, value, crc) = (28864..28864 + 232, 29039, 29108..29112);
    assert_eq!(
        bytes[value - 4..value + 4],
        [0x05, 0x37, 0x03, 0x00, 1, 0, 0, 0]
    );
    assert_eq!(
        bytes[crc.clone()],
        pst_crc(&bytes[block.clone()]).to_le_bytes()
    );
    bytes[value] = method;
    let sum = pst_crc(&bytes[block]).to_le_bytes();
    bytes[crc].copy_from_slice(&sum);
    built(&format!("mailbox-method-{method}.pst"), &bytes)
}

/// The message 08-forward-attached-message.eml became, in mailbox.pst.
const NOTE_G_MESSAGE: u64 = 2097380;

/// A by-reference attachment is listed with no file and no size, and nothing is written for it.
#[test]
fn a_by_reference_attachment_has_no_file() {
    let dir = scratch("by-reference");
    let store = with_attach_method(2);
    let out = run(&[store.as_os_str(), "--attachments".as_ref(), dir.as_os_str()]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(out.stderr, "");
    let lines = out.lines();
    let message = lines.iter().find(|l| l["id"] == NOTE_G_MESSAGE).unwrap();
    assert_eq!(
        message["attachments"],
        json!([{
            "filename": "note-g.txt",
            "mime_type": "text/plain",
            "inline": false,
            "method": "by_reference",
        }])
    );
    assert_eq!(names(&dir).len(), files_named(&lines).len());
}

/// An attachment outlook-pst can't read, here an afStorage one whose data isn't a storage, is a
/// skipped line after its message's line, with a warning, and the run exits 0.
#[test]
fn an_attachment_that_cant_be_read_is_skipped() {
    let out = run(&[with_attach_method(6)]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    let lines = out.lines();
    let at = lines
        .iter()
        .position(|l| l["line"] == "message" && l["id"] == NOTE_G_MESSAGE)
        .unwrap();
    assert_eq!(lines[at].get("attachments"), None);
    let reason = "Invalid PidTagAttachDataObject on afEmbeddedMessage attachment: Binary";
    assert_eq!(
        lines[at + 1],
        json!({
            "line": "skipped",
            "kind": "attachment",
            "folder": lines[at]["folder"],
            "message": NOTE_G_MESSAGE,
            "id": 5,
            "reason": reason,
        })
    );
    assert_eq!(
        out.stderr,
        format!(
            "spike-ai-pipeline-mail: warning: /Top of Personal Folders/Sent Items, message \
             {NOTE_G_MESSAGE}, attachment 5: skipped: {reason}\n"
        )
    );
}

/// Each generated store's and .msg's stdout equals its golden, byte for byte, so a regenerated
/// file that reads differently shows here, though its bytes always differ (manifest.json's
/// note). With UPDATE_GOLDEN set, the goldens are rewritten instead; review their diff before
/// committing.
#[test]
fn outputs_match_their_goldens() {
    let goldens = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/generated");
    let layout = json_file("layout.json");
    let stores = layout["stores"]
        .as_array()
        .unwrap()
        .iter()
        .map(|store| store["file"].as_str().unwrap().to_owned());
    for file in stores.chain(msg_files()) {
        let out = run(&[generated(&file)]);
        let golden = goldens.join(Path::new(&file).with_extension("jsonl"));
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::create_dir_all(golden.parent().unwrap()).unwrap();
            std::fs::write(&golden, &out.stdout).unwrap();
            continue;
        }
        assert!(
            out.stdout == std::fs::read_to_string(&golden).unwrap(),
            "{file}: stdout differs from {}",
            golden.display()
        );
    }
}

/// The non-ASCII sources cover more than one charset, and a non-ASCII folder name is in a layout.
#[test]
fn the_truth_covers_several_charsets() {
    let truth = json_file("truth.json");
    let charsets: BTreeSet<&str> = truth["messages"]
        .as_object()
        .unwrap()
        .values()
        .filter_map(|message| message.get("charset")?.as_str())
        .collect();
    assert!(charsets.len() >= 3, "{charsets:?}");
    let layout = json_file("layout.json");
    assert!(
        layout["stores"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|store| store["folders"].as_array().unwrap())
            .any(|folder| strings(&folder["path"]).iter().any(|n| !n.is_ascii()))
    );
}

/// Every source and output the manifest lists has the SHA-256 it records, every committed
/// source and output is listed, and every source has its truth.
#[test]
fn the_manifest_matches_the_committed_files() {
    let manifest = json_file("manifest.json");
    let truth = json_file("truth.json");
    let sources = manifest["sources"].as_array().unwrap();
    let outputs = manifest["outputs"].as_array().unwrap();
    let mut listed = BTreeSet::new();
    for entry in sources.iter().chain(outputs) {
        let file = entry["file"].as_str().unwrap();
        let path = generated(file);
        assert_eq!(
            Some(sha256(&path).as_str()),
            entry["sha256"].as_str(),
            "{file}"
        );
        listed.insert(file.to_owned());
    }
    for output in outputs {
        let file = output["file"].as_str().unwrap();
        let size = std::fs::metadata(generated(file)).unwrap().len();
        assert_eq!(Some(size), output["size"].as_u64(), "{file}: size");
        assert!(output["byte_stable"].is_boolean(), "{file}: byte_stable");
        for source in strings(&output["sources"]) {
            assert!(listed.contains(&source), "{file}: {source} isn't listed");
        }
    }
    let mut committed = BTreeSet::new();
    for dir in ["eml", "items", "pst", "msg"] {
        for entry in std::fs::read_dir(generated(dir)).unwrap() {
            let name = entry.unwrap().file_name().into_string().unwrap();
            committed.insert(format!("{dir}/{name}"));
        }
    }
    assert_eq!(listed, committed, "the manifest lists every committed file");
    let truths: BTreeSet<String> = [("messages", "eml"), ("items", "items")]
        .iter()
        .flat_map(|(section, dir)| {
            truth[*section]
                .as_object()
                .unwrap()
                .keys()
                .map(move |name| format!("{dir}/{name}"))
        })
        .collect();
    let sources: BTreeSet<String> = sources
        .iter()
        .map(|s| s["file"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(truths, sources, "every source has its truth");
}

/// The conversation store holds what the email package's conversation records are tested over: in
/// its Conversation History folder, a one-to-one and a multi-party Skype for Business chat, a
/// missed IM, and an ordinary IPM.Note for contrast, each item with a sender, a recipient and an
/// HTML body, and its end no earlier than its start.
#[test]
fn the_conversation_truth_covers_its_classes() {
    let layout = json_file("layout.json");
    let truth = json_file("truth.json");
    let store = layout["stores"]
        .as_array()
        .unwrap()
        .iter()
        .find(|store| store["file"] == "pst/conversations.pst")
        .unwrap();
    let folder = &store["folders"][0];
    assert_eq!(strings(&folder["path"]), ["Conversation History"]);
    let items: Vec<&Value> = strings(&folder["items"])
        .iter()
        .map(|source| &truth["items"][source])
        .collect();
    let of = |class: &str| -> Vec<&Value> {
        items
            .iter()
            .copied()
            .filter(|item| item["class"] == class)
            .collect()
    };
    let chats = of("IPM.Note.Microsoft.Conversation");
    let to = |item: &Value| item["to"].as_array().unwrap().len();
    assert!(chats.iter().any(|chat| to(chat) == 1), "a one-to-one chat");
    assert!(chats.iter().any(|chat| to(chat) > 1), "a multi-party chat");
    assert!(!of("IPM.Note.Microsoft.Missed").is_empty(), "a missed IM");
    assert!(!of("IPM.Note").is_empty(), "an IPM.Note");
    for item in items {
        assert!(item["sender"]["address"].is_string());
        assert!(to(item) >= 1);
        assert!(item["submit_time"].as_str() <= item["delivery_time"].as_str());
        assert!(item["html"].as_str().unwrap().starts_with("<html>"));
    }
}

/// The manifest's package versions are the ones generator/Generator.csproj pins, so the committed
/// outputs were built by the pinned generator.
#[test]
fn the_manifest_records_the_pinned_tools() {
    let manifest = json_file("manifest.json");
    let project = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("generator/Generator.csproj"),
    )
    .unwrap();
    let mut pins = 0;
    for line in project.lines() {
        let Some(rest) = line.trim().strip_prefix("<PackageReference Include=\"") else {
            continue;
        };
        let (package, rest) = rest.split_once('"').unwrap();
        let version = rest
            .split_once("Version=\"[")
            .and_then(|(_, v)| v.split_once("]\""))
            .map(|(v, _)| v)
            .unwrap_or_else(|| panic!("{package} isn't pinned exactly"));
        assert_eq!(manifest["tools"][package], version, "{package}");
        pins += 1;
    }
    assert!(
        pins >= 3,
        "PstBuilder, PstBuilder.Eml and MsgKit are pinned"
    );
    for tool in ["dotnet-sdk", "dotnet-runtime"] {
        assert!(manifest["tools"][tool].is_string(), "{tool}");
    }
}

/// The generated .msg files, by path under tests/fixtures/generated, one per layout.json source.
fn msg_files() -> Vec<String> {
    strings(&json_file("layout.json")["msg"])
        .iter()
        .map(|source| format!("msg/{}", source.replace(".eml", ".msg")))
        .collect()
}

/// An Internet message ID as MsgKit stores it: without its angle brackets.
fn bare(id: &str) -> Value {
    json!(id.trim_start_matches('<').trim_end_matches('>'))
}

/// The attachment types MsgKit 3.0.6 writes in place of the source's, by filename: it types an
/// attachment by its own table of extensions, which has no .csv.
const MSGKIT_MIME_TYPES: &[(&str, &str)] = &[("отчёт.csv", "application/octet-stream")];

/// One .msg message line against its source's truth, as the header comment says MsgKit carries
/// it. The attached message's .eml file is checked by its own test, so its record is taken as
/// read here but for its name, type and method.
fn assert_msg_message(got: &Value, truth: &Value, source: &str) {
    let expect = |field: &str, value: Option<&Value>| {
        assert_eq!(got.get(field), value, "{source}: {field}");
    };
    let smtp = |address: &Value| {
        let mut address = address.clone();
        address["address_type"] = json!("SMTP");
        address
    };
    expect("class", Some(&json!("IPM.Note")));
    expect("subject", truth.get("subject"));
    expect("sender", truth.get("sender").map(smtp).as_ref());
    expect("to", recipients(truth.get("to")).as_ref());
    expect("cc", recipients(truth.get("cc")).as_ref());
    expect("bcc", recipients(truth.get("bcc")).as_ref());
    // MsgKit takes the Date header for both the submit and the delivery time, and its time of
    // conversion for the creation and modification times.
    let date = &truth["date"];
    expect("submit_time", Some(date));
    expect("delivery_time", Some(date));
    assert_eq!(
        got["creation_time"], got["modification_time"],
        "{source}: creation and modification times"
    );
    assert_ne!(&got["creation_time"], date, "{source}: creation time");
    expect(
        "importance",
        Some(truth.get("importance").unwrap_or(&json!("normal"))),
    );
    expect("sensitivity", None);
    let id = |field: &str| truth.get(field).map(|id| bare(id.as_str().unwrap()));
    expect("message_id", id("message_id").as_ref());
    expect("in_reply_to", id("in_reply_to").as_ref());
    let last_reference = truth.get("references").map(|references| {
        bare(
            references
                .as_str()
                .unwrap()
                .split_whitespace()
                .last()
                .unwrap(),
        )
    });
    expect("references", last_reference.as_ref());
    // The conversation topic is the subject less its reply or forward prefix.
    let subject = truth["subject"].as_str().unwrap();
    let topic = ["Re: ", "Fwd: "]
        .iter()
        .find_map(|prefix| subject.strip_prefix(prefix))
        .unwrap_or(subject);
    expect("conversation_topic", Some(&json!(topic)));
    expect("conversation_index", None);
    // MimeKit rewrites the transport headers, so only the Message-ID's line is checked.
    let headers = got["transport_headers"].as_str().unwrap();
    let message_id = truth["message_id"].as_str().unwrap();
    assert!(
        headers.contains(&format!("Message-ID: {message_id}\n")),
        "{source}: transport headers"
    );

    let mut expected: Vec<Value> = attachments(truth)
        .map(|list| list.as_array().unwrap().clone())
        .unwrap_or_default();
    for record in &mut expected {
        if let Some((_, mime)) = MSGKIT_MIME_TYPES
            .iter()
            .find(|(name, _)| record["filename"] == *name)
        {
            record["mime_type"] = json!(mime);
        }
    }
    let embedded = truth.get("embedded").and_then(Value::as_array);
    for message in embedded.into_iter().flatten() {
        let read = &got["attachments"][expected.len()];
        expected.push(json!({
            "file": read["file"],
            "filename": format!("{}.eml", message["subject"].as_str().unwrap()),
            "mime_type": "message/rfc822",
            "inline": false,
            "method": "by_value",
            "size": read["size"],
        }));
    }
    expect(
        "attachments",
        (!expected.is_empty())
            .then_some(Value::Array(expected))
            .as_ref(),
    );

    for body in ["text", "html"] {
        let got = got["body"]
            .get(body)
            .map(|text| text.as_str().unwrap().replace("\r\n", "\n"));
        assert_eq!(
            got.as_deref(),
            truth["body"].get(body).map(|t| t.as_str().unwrap()),
            "{source}: body {body}"
        );
    }
    assert_eq!(got["body"].get("rtf"), None, "{source}: body rtf");
    let derived = got["body"]["derived"].as_object().unwrap();
    assert_eq!(
        derived.keys().collect::<Vec<_>>(),
        ["markdown"],
        "{source}: derived bodies"
    );
}

/// Each generated .msg, read through the binary, is a store line and one message line that
/// matches its source's truth, without a warning.
#[test]
fn msg_files_match_the_eml_truth() {
    let truth = json_file("truth.json");
    let sources = strings(&json_file("layout.json")["msg"]);
    let mut created = BTreeSet::new();
    for (file, source) in msg_files().iter().zip(&sources) {
        let out = run(&[generated(file)]);
        assert_eq!(out.code, Some(0), "{file}: {}", out.stderr);
        assert_eq!(out.stderr, "", "{file}");
        let lines = out.lines();
        assert_eq!(lines.len(), 2, "{file}");
        assert_eq!(
            lines[0],
            json!({"line": "store", "schema": "1", "format": "msg"}),
            "{file}"
        );
        assert_eq!(lines[1]["line"], "message", "{file}");
        assert_msg_message(&lines[1], &truth["messages"][source], source);
        created.insert(lines[1]["creation_time"].as_str().unwrap().to_owned());
    }
    // One generator run converted them all, within a second.
    assert_eq!(created.len(), 1, "{created:?}");
}

/// MsgKit 3.0.6 attaches a forwarded message (message/rfc822) as a by-value .eml file named by
/// its subject, not as an embedded message, so a generated .msg has none: the file holds the
/// source's attached message, which the truth's embedded entry describes. With --attachments,
/// each record's file is written by content.
#[test]
fn msgkit_attaches_a_forwarded_message_as_an_eml_file() {
    let truth = &json_file("truth.json")["messages"]["08-forward-attached-message.eml"];
    let embedded = &truth["embedded"][0];
    let dir = scratch("generated-msg-attachments");
    let file = generated("msg/08-forward-attached-message.msg");
    let out = run(&[file.as_os_str(), "--attachments".as_ref(), dir.as_os_str()]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    let lines = out.lines();
    let mut expected = files_named(&lines);
    expected.sort_unstable();
    assert_eq!(names(&dir), expected);
    for name in &expected {
        let (sha, _) = name.split_once('.').unwrap();
        assert_eq!(sha256(&dir.join(name)), sha, "{name}");
    }
    let record = &lines[1]["attachments"][1];
    assert_eq!(record["method"], "by_value");
    assert!(record.get("message").is_none());
    let eml = std::fs::read_to_string(dir.join(record["file"].as_str().unwrap()))
        .unwrap()
        .replace("\r\n", "\n");
    for line in [
        format!("Message-ID: {}", embedded["message_id"].as_str().unwrap()),
        format!("Subject: {}", embedded["subject"].as_str().unwrap()),
    ] {
        assert!(eml.contains(&format!("{line}\n")), "{line}: {eml}");
    }
    let (_, body) = eml.split_once("\n\n").unwrap();
    assert_eq!(body, embedded["body"]["text"]);
}
