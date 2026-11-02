//! The generated fixtures under tests/fixtures/generated: the .pst stores built from the
//! hand-written .eml sources, read through the binary and checked against the sources' truth, and
//! the manifest checked against the committed files.
//!
//! The truth (truth.json) is the sources'; what a store carries of it is stated here. PST Builder
//! 1.3.0, through its EmlMapper, writes the subject, the sender's name and address, the To and Cc
//! recipients, the times, the plain-text and HTML bodies and the attachments. It writes no Bcc, no
//! sender address type, no threading fields, no transport headers and no conversation index, and
//! it skips message/rfc822 parts, so a generated store has no embedded messages. Its importance
//! and sensitivity are always normal, and its conversation topic is the subject. It writes each
//! attachment by value, with no PidTagAttachExtension, and marks an inline one with
//! ATT_MHTML_REF. The HTML truth waits for the slice that reads it.

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
    let text = got["body"]
        .get("text")
        .map(|text| text.as_str().unwrap().replace("\r\n", "\n"));
    assert_eq!(
        text.as_deref(),
        truth["body"].get("text").map(|t| t.as_str().unwrap()),
        "{source}: body text"
    );
}

/// Each generated store, read through the binary, holds the layout's folder tree, and in each
/// folder its sources' messages in order, each matching its source's truth.
#[test]
fn stores_match_the_eml_truth() {
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
            let sources = strings(&folder["messages"]);
            let got = messages.remove(&path).unwrap_or_default();
            assert_eq!(got.len(), sources.len(), "{file}: messages in {path:?}");
            for (message, source) in got.iter().zip(&sources) {
                assert_message(message, &truth["messages"][source], source);
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
/// bytes those its name hashes; stdout is the same as without the flag.
#[test]
fn attachments_are_written_by_content() {
    for store in json_file("layout.json")["stores"].as_array().unwrap() {
        let file = generated(store["file"].as_str().unwrap());
        let stem = file.file_stem().unwrap().to_str().unwrap();
        let dir = scratch(&format!("generated-attachments-{stem}")).join("created");
        let out = run(&[file.as_os_str(), "--attachments".as_ref(), dir.as_os_str()]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, run(&[&file]).stdout);
        let mut expected = files_named(&out.lines());
        assert!(!expected.is_empty());
        expected.sort_unstable();
        expected.dedup();
        assert_eq!(names(&dir), expected);
        for name in expected {
            let (sha, _) = name.split_once('.').unwrap();
            assert_eq!(sha256(&dir.join(&name)), sha, "{name}");
        }
    }
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

/// Each generated store's stdout equals its golden, byte for byte, so a regenerated store that
/// reads differently shows here, though its bytes always differ (manifest.json's note). With
/// UPDATE_GOLDEN set, the goldens are rewritten instead; review their diff before committing.
#[test]
fn stores_match_their_goldens() {
    let goldens = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/generated");
    let layout = json_file("layout.json");
    for store in layout["stores"].as_array().unwrap() {
        let file = store["file"].as_str().unwrap();
        let out = run(&[generated(file)]);
        let golden = goldens.join(Path::new(file).with_extension("jsonl").file_name().unwrap());
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::create_dir_all(&goldens).unwrap();
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
    for dir in ["eml", "pst", "msg"] {
        for entry in std::fs::read_dir(generated(dir)).unwrap() {
            let name = entry.unwrap().file_name().into_string().unwrap();
            committed.insert(format!("{dir}/{name}"));
        }
    }
    assert_eq!(listed, committed, "the manifest lists every committed file");
    let truths: BTreeSet<String> = truth["messages"]
        .as_object()
        .unwrap()
        .keys()
        .map(|name| format!("eml/{name}"))
        .collect();
    let sources: BTreeSet<String> = sources
        .iter()
        .map(|s| s["file"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(truths, sources, "every source has its truth");
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

/// The generated .msg files are committed for the .msg reader to come; until then the CLI sniffs
/// each as a compound file and refuses it.
#[test]
fn generated_msg_files_are_refused_for_now() {
    let manifest = json_file("manifest.json");
    for output in manifest["outputs"].as_array().unwrap() {
        let file = output["file"].as_str().unwrap();
        if !file.ends_with(".msg") {
            continue;
        }
        let out = run(&[generated(file)]);
        assert_eq!(out.code, Some(3), "{file}");
        assert!(
            out.stderr
                .contains("a compound file, as a .msg is; .msg is not yet supported"),
            "{file}: {}",
            out.stderr
        );
    }
}
