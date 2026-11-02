//! The generated fixtures under tests/fixtures/generated: the .pst stores built from the
//! hand-written .eml sources, read through the binary and checked against the sources' truth, and
//! the manifest checked against the committed files.
//!
//! The truth (truth.json) is the sources'; what a store carries of it is stated here. PST Builder
//! 1.3.0, through its EmlMapper, writes the subject, the sender's name and address, the To and Cc
//! recipients, the times, the plain-text and HTML bodies and the attachments. It writes no Bcc, no
//! sender address type, no threading fields, no transport headers and no conversation index, and
//! it skips message/rfc822 parts. Its importance and sensitivity are always normal, and its
//! conversation topic is the subject. The attachment and HTML truth waits for the slices that read
//! them.

mod common;

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use common::{fixture, run, sha256};
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
