//! mbox files through the binary: a store line, then each message in file order, its id its
//! ordinal, with a skipped line in place of a message that can't be read. The MIME group's
//! mboxrd, tests/fixtures/mime/nine.mboxrd, is rebuilt by scripts/mboxrd.sh and compared byte
//! for byte, and each of its messages is compared with the CLI's read of its source .eml. Apache
//! James mime4j's mbox.rlug, under tests/fixtures/mime/rlug/, is compared with its five expected
//! splits the same way.

mod common;

use std::path::PathBuf;
use std::process::Command;

use common::{built, fixture, run, scratch, sha256};
use serde_json::{Value, json};

/// The mboxrd's sources, in its order, under tests/fixtures.
const SOURCES: &[&str] = &[
    "generated/eml/01-plain.eml",
    "generated/eml/02-reply-latin1.eml",
    "generated/eml/03-html-inline-image.eml",
    "generated/eml/04-attachment-cyrillic.eml",
    "generated/eml/05-japanese.eml",
    "generated/eml/06-chinese.eml",
    "generated/eml/07-html-only-bcc.eml",
    "generated/eml/08-forward-attached-message.eml",
    "mime/eml/09-threading.eml",
];

/// mime4j's mbox.rlug and its expected splits, in the MIME group.
const RLUG: &[&str] = &[
    "rlug/mbox.rlug",
    "rlug/mbox.rlug-0",
    "rlug/mbox.rlug-1",
    "rlug/mbox.rlug-2",
    "rlug/mbox.rlug-3",
    "rlug/mbox.rlug-4",
];

/// The committed mboxrd.
fn mboxrd() -> PathBuf {
    fixture("mime/nine.mboxrd")
}

/// A run's lines but its store line, each without its id, and the ids apart.
fn without_ids(lines: &[Value]) -> (Vec<Value>, Vec<Value>) {
    let mut ids = Vec::new();
    let lines = lines[1..]
        .iter()
        .map(|line| {
            let mut line = line.clone();
            if let Some(id) = line.as_object_mut().unwrap().remove("id") {
                ids.push(id);
            }
            line
        })
        .collect();
    (lines, ids)
}

/// The script rebuilds the committed mboxrd byte for byte.
#[test]
fn the_mboxrd_is_rebuilt_byte_for_byte() {
    let rebuilt = scratch("mboxrd-rebuilt").join("nine.mboxrd");
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/mboxrd.sh");
    let status = Command::new(script).arg(&rebuilt).status().unwrap();
    assert!(status.success());
    assert!(
        std::fs::read(&rebuilt).unwrap() == std::fs::read(mboxrd()).unwrap(),
        "scripts/mboxrd.sh no longer writes tests/fixtures/mime/nine.mboxrd; run mise run mboxrd"
    );
}

/// Each message of the mboxrd is the CLI's read of its source .eml, its attachments too, with
/// ids 1 to 9.
#[test]
fn the_mboxrd_gives_its_sources_by_ordinal() {
    let dir = scratch("mboxrd-attachments");
    let out = run(&[
        mboxrd().as_os_str(),
        "--attachments".as_ref(),
        dir.as_os_str(),
    ]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(out.stderr, "");
    let lines = out.lines();
    assert_eq!(lines[0]["format"], "mbox");
    let (messages, ids) = without_ids(&lines);
    assert_eq!(ids, (1..=9).map(Value::from).collect::<Vec<_>>());
    let mut expected = Vec::new();
    for (n, source) in SOURCES.iter().enumerate() {
        let dir = scratch(&format!("mboxrd-source-{n}"));
        let out = run(&[
            fixture(source).as_os_str(),
            "--attachments".as_ref(),
            dir.as_os_str(),
        ]);
        assert_eq!(out.code, Some(0), "{source}: {}", out.stderr);
        let lines = out.lines();
        assert!(
            lines
                .iter()
                .all(|line| line["line"] == "message" || line["line"] == "store"),
            "{source}: a skipped line has no counterpart"
        );
        expected.extend(without_ids(&lines).0);
    }
    assert_eq!(messages, expected);
}

/// 09's body lines starting "From " and ">From ", quoted in the mboxrd, are read back as they
/// were.
#[test]
fn from_lines_survive_the_mboxrd() {
    let out = run(&[mboxrd()]);
    let lines = out.lines();
    let text = lines[9]["body"]["text"].as_str().unwrap();
    assert!(text.contains("\nFrom the table in Note G"), "{text}");
    assert!(text.contains("\n>From your last letter"), "{text}");
}

#[test]
fn an_mbox_gives_its_messages_by_ordinal() {
    let mbox = built(
        "three.mbox",
        b"From ann@example.com Mon Jan 12 17:00:00 2026\n\
          From: ann@example.com\n\
          Subject: first\n\
          \n\
          >From the start\n\
          From bob@example.com Mon Jan 12 17:01:00 2026\n\
          no header here\n\
          From cy@example.com Mon Jan 12 17:02:00 2026\n\
          Date: Mon, 12 Jan 2026 17:02:00 +0000\n\
          Subject: third\n\
          \n\
          body\n",
    );
    let out = run(&[&mbox]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    let lines = out.lines();
    let summary: Vec<_> = lines
        .iter()
        .map(|line| {
            json!([
                line["line"],
                line.get("id"),
                line.get("subject"),
                line.get("folder")
            ])
        })
        .collect();
    assert_eq!(
        summary,
        [
            json!(["store", null, null, null]),
            json!(["message", 1, "first", null]),
            json!(["skipped", 2, null, null]),
            json!(["message", 3, "third", null]),
        ]
    );
    assert_eq!(lines[0]["format"], "mbox");
    assert_eq!(lines[2]["kind"], "message");
    assert_eq!(
        out.stderr,
        "spike-ai-pipeline-mail: warning: message 2: \
         skipped: it has no header block with a From or Date field\n"
    );
}

/// A file of separator lines alone has no messages: the store line alone.
#[test]
fn an_mbox_with_no_messages_gives_its_store_line_alone() {
    let mbox = built(
        "empty.mbox",
        b"From MAILER-DAEMON Thu Jan  1 00:00:00 1970\n\n",
    );
    let out = run(&[&mbox]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(out.stderr, "");
    let lines = out.lines();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["line"], "store");
    assert_eq!(lines[0]["format"], "mbox");
}

/// An empty entry, a separator line with nothing after it, keeps its ordinal but writes no
/// line, so the message after it keeps its own ordinal.
#[test]
fn an_empty_entry_keeps_its_ordinal() {
    let mbox = built(
        "empty-entry.mbox",
        b"From ann@example.com Mon Jan 12 17:00:00 2026\n\
          From: ann@example.com\n\
          Subject: first\n\
          \n\
          body\n\
          \n\
          From MAILER-DAEMON Thu Jan  1 00:00:00 1970\n\
          \n\
          From cy@example.com Mon Jan 12 17:02:00 2026\n\
          From: cy@example.com\n\
          Subject: third\n\
          \n\
          body\n\
          \n",
    );
    let out = run(&[&mbox]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(out.stderr, "");
    let summary: Vec<_> = out
        .lines()
        .iter()
        .map(|line| json!([line["line"], line.get("id"), line.get("subject")]))
        .collect();
    assert_eq!(
        summary,
        [
            json!(["store", null, null]),
            json!(["message", 1, "first"]),
            json!(["message", 3, "third"]),
        ]
    );
}

/// mboxo quotes only "From " lines, and is read as mboxrd: a body line that was ">From " to begin
/// with, which mboxo leaves as it is, loses its ">" (a known limit).
#[test]
fn an_mboxo_body_line_that_was_quoted_loses_its_quote() {
    let mbox = built(
        "mboxo.mbox",
        b"From ann@example.com Mon Jan 12 17:00:00 2026\n\
          From: ann@example.com\n\
          \n\
          >From a line that was \"From \"\n\
          >From a line that was \">From \"\n\
          \n",
    );
    let out = run(&[&mbox]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    let lines = out.lines();
    assert_eq!(
        lines[1]["body"]["text"],
        "From a line that was \"From \"\nFrom a line that was \">From \"\n"
    );
}

/// mboxcl's Content-Length is not read (a known limit): an unquoted body line starting "From "
/// starts a new message, which here fails the header-shape check.
#[test]
fn an_mboxcl_content_length_is_ignored() {
    let mbox = built(
        "mboxcl.mbox",
        b"From ann@example.com Mon Jan 12 17:00:00 2026\n\
          From: ann@example.com\n\
          Content-Length: 44\n\
          \n\
          first line\n\
          From the second line\n\
          and a third\n\
          \n",
    );
    let out = run(&[&mbox]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    let lines = out.lines();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[1]["id"], 1);
    assert_eq!(lines[1]["body"]["text"], "first line\n");
    assert_eq!(lines[2]["line"], "skipped");
    assert_eq!(lines[2]["kind"], "message");
    assert_eq!(lines[2]["id"], 2);
    assert_eq!(
        out.stderr,
        "spike-ai-pipeline-mail: warning: message 2: \
         skipped: it has no header block with a From or Date field\n"
    );
}

/// mime4j's mbox.rlug gives five messages, ids 1 to 5, each the CLI's read of its expected split
/// as an .eml, every field but the id alike.
///
/// Each split is exactly the bytes between two separator lines, so it ends with the blank line
/// before the next separator, or the file's last line. RFC 4155 and mboxrd give that blank line
/// to the separator, and the reader drops it; mime4j keeps it in the message. So each split is
/// read without its final line ending, and that line ending is checked to be "\n" alone, so the
/// comparison can't hide any other difference.
#[test]
fn mbox_rlug_gives_its_splits_by_ordinal() {
    let out = run(&[fixture("mime/rlug/mbox.rlug")]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(out.stderr, "");
    let lines = out.lines();
    assert_eq!(lines[0]["format"], "mbox");
    let (messages, ids) = without_ids(&lines);
    assert_eq!(ids, (1..=5).map(Value::from).collect::<Vec<_>>());
    let mut expected = Vec::new();
    for split in &RLUG[1..] {
        let bytes = std::fs::read(fixture(&format!("mime/{split}"))).unwrap();
        let message = bytes
            .strip_suffix(b"\n")
            .unwrap_or_else(|| panic!("{split} ends with a line ending"));
        assert!(message.ends_with(b"\n"), "{split} ends with a blank line");
        let name = split.rsplit('/').next().unwrap();
        let out = run(&[built(&format!("{name}.eml"), message)]);
        assert_eq!(out.code, Some(0), "{split}: {}", out.stderr);
        assert_eq!(out.stderr, "", "{split}");
        let lines = out.lines();
        assert_eq!(lines.len(), 2, "{split}: one message line");
        expected.extend(without_ids(&lines).0);
    }
    assert_eq!(messages, expected);
}

/// Every committed mime4j file has a sources entry whose SHA-256 matches it, and the license
/// notices it names are committed.
#[test]
fn every_rlug_file_has_a_matching_sources_entry() {
    let sources: Value =
        serde_json::from_slice(&std::fs::read(fixture("mime/sources.json")).unwrap()).unwrap();
    let entries = sources.as_array().unwrap();
    let mut listed: Vec<&str> = entries
        .iter()
        .map(|e| e["file"].as_str().unwrap())
        .collect();
    listed.sort_unstable();
    let mut files = RLUG.to_vec();
    files.sort_unstable();
    assert_eq!(listed, files);
    for entry in entries {
        for field in ["repository", "commit", "path", "license", "sha256"] {
            assert!(entry[field].is_string(), "{}: {field}", entry["file"]);
        }
        let path = fixture(&format!("mime/{}", entry["file"].as_str().unwrap()));
        assert_eq!(
            Some(sha256(&path).as_str()),
            entry["sha256"].as_str(),
            "{}",
            path.display()
        );
    }
    for notice in ["rlug/LICENSE", "rlug/NOTICE"] {
        assert!(fixture(&format!("mime/{notice}")).is_file(), "{notice}");
    }
}
