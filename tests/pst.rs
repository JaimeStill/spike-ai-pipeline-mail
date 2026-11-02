//! Reading .pst stores: the committed public stores against their goldens and the hand truth,
//! and the skip policy on stores corrupted in the test.

mod common;

use std::path::Path;

use common::{built, fixture, run, sha256};
use serde_json::Value;

/// The committed public stores, by path under tests/fixtures/pst.
const STORES: &[&str] = &[
    "tika/testPST.pst",
    "tika/testPST_variousBodyTypes.pst",
    "libyal/outlook.pst",
    "java-libpst/dist-list.pst",
    "java-libpst/passworded.pst",
];

/// Each store's stdout and stderr equal its goldens, byte for byte, and it exits 0. With
/// UPDATE_GOLDEN set, the goldens are rewritten instead; review their diff before committing.
#[test]
fn stores_match_their_goldens() {
    let goldens = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/pst");
    for store in STORES {
        let out = run(&[fixture(&format!("pst/{store}"))]);
        assert_eq!(out.code, Some(0), "{store}: {}", out.stderr);
        let stem = store.replace('/', "-").replace(".pst", "");
        let (stdout, stderr) = (
            goldens.join(format!("{stem}.jsonl")),
            goldens.join(format!("{stem}.stderr")),
        );
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::create_dir_all(&goldens).unwrap();
            std::fs::write(&stdout, &out.stdout).unwrap();
            std::fs::write(&stderr, &out.stderr).unwrap();
            continue;
        }
        assert!(
            out.stdout == std::fs::read_to_string(&stdout).unwrap(),
            "{store}: stdout differs from {}",
            stdout.display()
        );
        assert_eq!(
            out.stderr,
            std::fs::read_to_string(&stderr).unwrap(),
            "{store}: stderr"
        );
    }
}

/// The same input always gives byte-identical output.
#[test]
fn output_is_byte_identical_across_runs() {
    for store in STORES {
        let path = fixture(&format!("pst/{store}"));
        assert_eq!(run(&[&path]).stdout, run(&[&path]).stdout, "{store}");
    }
}

/// The shape of schema "1": the store line first with its facts, then folders depth-first, each
/// before its messages; every message names a folder already written; no field is null.
#[test]
fn lines_follow_schema_1() {
    for store in STORES {
        let lines = run(&[fixture(&format!("pst/{store}"))]).lines();
        let first = &lines[0];
        assert_eq!(first["line"], "store", "{store}");
        assert_eq!(first["schema"], "1", "{store}");
        assert_eq!(first["format"], "pst", "{store}");
        assert_eq!(first["encoding"], "unicode", "{store}");
        let mut folders: Vec<(u64, Vec<Value>)> = Vec::new();
        for line in &lines[1..] {
            assert_no_nulls(line, store);
            match line["line"].as_str().unwrap() {
                "folder" => {
                    let path = line["path"].as_array().unwrap().clone();
                    if let Some(parent) = line.get("parent") {
                        // Depth-first: the parent is on the path back from the last folder.
                        let (_, parent_path) = folders
                            .iter()
                            .rev()
                            .find(|(id, _)| Some(*id) == parent.as_u64())
                            .expect("a folder's parent is written before it");
                        assert_eq!(parent_path[..], path[..path.len() - 1], "{store}");
                        let last = &folders.last().unwrap().1;
                        assert!(last.starts_with(parent_path), "{store}: not depth-first");
                    } else {
                        assert!(folders.is_empty(), "{store}: only the root has no parent");
                    }
                    folders.push((line["id"].as_u64().unwrap(), path));
                }
                // A skipped folder names its parent, whose line is written but may not be the
                // last: the parent's earlier subfolders come between.
                "skipped" if line["kind"] == "folder" => {
                    assert!(
                        folders
                            .iter()
                            .any(|(id, _)| Some(*id) == line["folder"].as_u64()),
                        "{store}: a skipped folder's parent is written before it"
                    );
                }
                "message" | "skipped" => {
                    let folder = line["folder"].as_u64();
                    assert_eq!(
                        folders.last().map(|(id, _)| *id),
                        folder,
                        "{store}: a message follows its own folder's line"
                    );
                }
                other => panic!("{store}: unknown line {other}"),
            }
        }
    }
}

fn assert_no_nulls(value: &Value, store: &str) {
    match value {
        Value::Null => panic!("{store}: a null was written"),
        Value::Array(items) => items.iter().for_each(|item| assert_no_nulls(item, store)),
        Value::Object(fields) => fields
            .values()
            .for_each(|item| assert_no_nulls(item, store)),
        _ => {}
    }
}

/// The CLI's threading fields equal the hand truth, message by message, in store order.
#[test]
fn threading_fields_match_the_tika_truth() {
    let truth: Value =
        serde_json::from_slice(&std::fs::read(fixture("pst/tika-threading.json")).unwrap())
            .unwrap();
    for (store, expected) in truth["stores"].as_object().unwrap() {
        let lines = run(&[fixture(&format!("pst/{store}"))]).lines();
        let messages: Vec<&Value> = lines.iter().filter(|l| l["line"] == "message").collect();
        let expected = expected.as_array().unwrap();
        assert_eq!(messages.len(), expected.len(), "{store}");
        for (message, truth) in messages.iter().zip(expected) {
            for (field, value) in truth.as_object().unwrap() {
                let got = message.get(field).unwrap_or(&Value::Null);
                assert_eq!(got, value, "{store}: {} {field}", truth["subject"]);
            }
        }
    }
}

/// The first message of Tika's testPST.pst, against what Tika's own test asserts of it
/// (OutlookPSTParserTest at apache/tika 48010f8).
#[test]
fn the_first_tika_message_matches_tikas_assertions() {
    let lines = run(&[fixture("pst/tika/testPST.pst")]).lines();
    let first = lines.iter().find(|l| l["line"] == "message").unwrap();
    assert_eq!(first["class"], "IPM.Note");
    assert_eq!(first["subject"], "Re: Feature Generators");
    assert_eq!(first["sender"]["name"], "Jörn Kottmann");
    assert_eq!(first["sender"]["address"], "kottmann@gmail.com");
    assert_eq!(first["to"][0]["name"], "users@opennlp.apache.org");
    assert_eq!(first.get("cc"), None);
    assert_eq!(first.get("bcc"), None);
    assert_eq!(first["submit_time"], "2014-02-26T07:50:04Z");
    assert_eq!(first["delivery_time"], "2014-02-26T07:51:02Z");
    assert_eq!(first["creation_time"], "2014-02-26T07:51:02Z");
    assert_eq!(first["modification_time"], "2014-02-26T12:37:43Z");
    let folder = &lines[1];
    assert_eq!(
        folder["display_name"],
        "Début du fichier de données Outlook"
    );
}

#[test]
fn the_store_line_records_the_password() {
    for (store, password) in [
        ("java-libpst/passworded.pst", true),
        ("java-libpst/dist-list.pst", false),
    ] {
        let lines = run(&[fixture(&format!("pst/{store}"))]).lines();
        assert_eq!(lines[0]["password"], password, "{store}");
    }
}

/// outlook-pst 1.2.0 can't read a message node without a sub-node tree; the contact in
/// java-libpst's stores that has none becomes a skipped line and a warning, and the run exits 0.
#[test]
fn a_message_outlook_pst_cannot_read_is_skipped() {
    for store in ["java-libpst/dist-list.pst", "java-libpst/passworded.pst"] {
        let out = run(&[fixture(&format!("pst/{store}"))]);
        assert_eq!(out.code, Some(0));
        let skipped: Vec<Value> = out
            .lines()
            .into_iter()
            .filter(|l| l["line"] == "skipped")
            .collect();
        assert_eq!(
            skipped,
            [serde_json::json!({
                "line": "skipped",
                "kind": "message",
                "folder": 33090,
                "id": 2097188,
                "reason": "Missing Sub-Node Tree on message",
            })],
            "{store}"
        );
        assert_eq!(
            out.stderr,
            "spike-ai-pipeline-mail: warning: /Top of Personal Folders/Contacts, message \
             2097188: skipped: Missing Sub-Node Tree on message\n"
        );
    }
}

/// A copy of libyal's store with 32 bytes overwritten at `offset`.
fn corrupted(name: &str, offset: usize) -> std::path::PathBuf {
    let mut bytes = std::fs::read(fixture("pst/libyal/outlook.pst")).unwrap();
    bytes[offset..offset + 32].fill(0xFF);
    built(name, &bytes)
}

/// A corrupt record inside a readable store is skipped, never fatal: each skipped line has its
/// warning, and the records around it are still written.
#[test]
fn corrupt_records_are_skipped_and_the_run_exits_0() {
    // A node B-tree page: two folders and the message under Inbox can't be found.
    let out = run(&[corrupted("corrupt-btree.pst", 18944)]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    let lines = out.lines();
    let skipped: Vec<(String, u64)> = lines
        .iter()
        .filter(|l| l["line"] == "skipped")
        .map(|l| {
            (
                l["kind"].as_str().unwrap().to_owned(),
                l["id"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        skipped,
        [
            ("folder".to_owned(), 32930),
            ("folder".to_owned(), 32962),
            ("message".to_owned(), 2097188),
            ("folder".to_owned(), 33122),
        ]
    );
    let warnings: Vec<&str> = out.stderr.lines().collect();
    assert_eq!(warnings.len(), skipped.len());
    assert!(warnings[0].starts_with(
        "spike-ai-pipeline-mail: warning: /Top of Personal Folders, folder 32930: skipped: "
    ));
    assert!(
        lines
            .iter()
            .any(|l| l["line"] == "message" && l["id"] == 2097220)
    );

    // A message's data block: only that message is skipped.
    let out = run(&[corrupted("corrupt-message.pst", 29696)]);
    assert_eq!(out.code, Some(0));
    assert_eq!(
        out.stderr,
        "spike-ai-pipeline-mail: warning: /Top of Personal Folders/Inbox, message 2097188: \
         skipped: Invalid internal block btype: 0xFF\n"
    );
    let messages = out
        .lines()
        .into_iter()
        .filter(|l| l["line"] == "message")
        .count();
    assert_eq!(messages, 1);
}

/// Every committed store has a sources entry whose SHA-256 matches it.
#[test]
fn every_store_has_a_matching_sources_entry() {
    let sources: Value =
        serde_json::from_slice(&std::fs::read(fixture("pst/sources.json")).unwrap()).unwrap();
    let entries = sources.as_array().unwrap();
    let mut listed: Vec<&str> = entries
        .iter()
        .map(|e| e["file"].as_str().unwrap())
        .collect();
    listed.sort_unstable();
    let mut stores = STORES.to_vec();
    stores.sort_unstable();
    assert_eq!(listed, stores);
    for entry in entries {
        for field in ["repository", "commit", "path", "license", "sha256"] {
            assert!(entry[field].is_string(), "{}: {field}", entry["file"]);
        }
        let path = fixture(&format!("pst/{}", entry["file"].as_str().unwrap()));
        assert_eq!(
            Some(sha256(&path).as_str()),
            entry["sha256"].as_str(),
            "{}",
            path.display()
        );
    }
}
