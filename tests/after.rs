//! `--after <id>`: a .pst read resumed after a message's node id, an mbox read after a message's
//! ordinal. The tail a resumed read writes is defined against the whole read of the same input:
//! every line that isn't a message's, the store line, folder lines and the skipped lines of
//! folders and folders' tables, as the whole read writes them; and of the message-level lines, a
//! message line and the skipped lines of a message and of its attachments, only those of the
//! messages after the cursor's, in the whole read's order. The tests build that tail from the
//! whole read's own lines and compare the resumed stdout with it byte for byte.

mod common;

use std::collections::BTreeSet;
use std::path::Path;

use common::{built, fixture, names, run, scratch};
use serde_json::Value;

/// Every committed .pst and mbox, under tests/fixtures.
const INPUTS: &[&str] = &[
    "pst/java-libpst/dist-list.pst",
    "pst/java-libpst/passworded.pst",
    "pst/libyal/outlook.pst",
    "pst/tika/testPST.pst",
    "pst/tika/testPST_variousBodyTypes.pst",
    "generated/pst/mailbox.pst",
    "mime/nine.mboxrd",
    "mime/rlug/mbox.rlug",
];

/// The message a line belongs to, by the message's id, or None for a line that isn't a
/// message's: a message line, a skipped message's, or a skipped attachment's, which names its
/// message.
fn owner(line: &Value) -> Option<u64> {
    match (line["line"].as_str(), line["kind"].as_str()) {
        (Some("message"), _) | (Some("skipped"), Some("message")) => line["id"].as_u64(),
        (Some("skipped"), Some("attachment")) => line["message"].as_u64(),
        _ => None,
    }
}

/// The message ids of a whole read, in its order.
fn messages(stdout: &str) -> Vec<u64> {
    let mut ids = Vec::new();
    for line in stdout.lines() {
        let line: Value = serde_json::from_str(line).unwrap();
        if let Some(id) = owner(&line)
            && ids.last() != Some(&id)
        {
            ids.push(id);
        }
    }
    ids
}

/// The whole read's tail after message `cursor`: its lines that are no message's, and the lines
/// of the messages after the cursor's.
fn tail(stdout: &str, cursor: u64) -> String {
    let mut seen = false;
    let mut passed = false;
    let mut kept = String::new();
    for raw in stdout.lines() {
        let line: Value = serde_json::from_str(raw).unwrap();
        let keep = match owner(&line) {
            None => true,
            Some(_) if passed => true,
            Some(id) if id == cursor => {
                seen = true;
                false
            }
            Some(_) => {
                passed = seen;
                passed
            }
        };
        if keep {
            kept.push_str(raw);
            kept.push('\n');
        }
    }
    assert!(seen, "message {cursor} is in the whole read");
    kept
}

/// The attachment files a read's lines name, nested messages' too.
fn files(stdout: &str) -> BTreeSet<String> {
    fn walk(message: &Value, files: &mut BTreeSet<String>) {
        for attachment in message["attachments"].as_array().into_iter().flatten() {
            if let Some(file) = attachment["file"].as_str() {
                files.insert(file.to_owned());
            }
            walk(&attachment["message"], files);
        }
    }
    let mut files = BTreeSet::new();
    for line in stdout.lines() {
        walk(&serde_json::from_str(line).unwrap(), &mut files);
    }
    files
}

/// For each committed .pst and mbox, resumed after its first, middle and last message: the
/// resumed stdout is the whole read's tail byte for byte; the attachments directory holds the
/// tail's files, each as the whole read wrote it, and none of the passed-over messages' alone;
/// and no warning names a passed-over message, since none is read.
#[test]
fn a_resumed_read_is_the_whole_reads_tail() {
    for (n, input) in INPUTS.iter().enumerate() {
        let path = fixture(input);
        let whole_dir = scratch(&format!("after-whole-{n}"));
        let whole = run(&[
            path.as_os_str(),
            "--attachments".as_ref(),
            whole_dir.as_os_str(),
        ]);
        assert_eq!(whole.code, Some(0), "{input}: {}", whole.stderr);
        let ids = messages(&whole.stdout);
        assert!(ids.len() >= 2, "{input} has a first and a last message");
        let cursors = BTreeSet::from([0, ids.len() / 2, ids.len() - 1]);
        for at in cursors {
            let cursor = ids[at];
            let dir = scratch(&format!("after-{n}-{cursor}"));
            let resumed = run(&[
                path.as_os_str(),
                "--after".as_ref(),
                cursor.to_string().as_ref(),
                "--attachments".as_ref(),
                dir.as_os_str(),
            ]);
            let what = format!("{input} --after {cursor}");
            assert_eq!(resumed.code, Some(0), "{what}: {}", resumed.stderr);
            assert!(
                resumed.stdout == tail(&whole.stdout, cursor),
                "{what}: the resumed stdout is not the whole read's tail"
            );
            let expected: Vec<String> = files(&resumed.stdout).into_iter().collect();
            assert_eq!(names(&dir), expected, "{what}");
            for file in &expected {
                assert!(
                    std::fs::read(dir.join(file)).unwrap()
                        == std::fs::read(whole_dir.join(file)).unwrap(),
                    "{what}: {file}"
                );
            }
            for passed in &ids[..=at] {
                for place in [format!("message {passed}:"), format!("message {passed},")] {
                    assert!(
                        !resumed.stderr.contains(&place),
                        "{what}: a warning names message {passed}: {}",
                        resumed.stderr
                    );
                }
            }
        }
    }
}

/// The last message's cursor leaves every folder line and no message line, and writes no
/// attachment: here the generated store, whose last message has one.
#[test]
fn after_the_last_message_only_the_folders_are_written() {
    let path = fixture("generated/pst/mailbox.pst");
    let whole = run(&[&path]);
    let last = *messages(&whole.stdout).last().unwrap();
    assert!(
        !files(&whole.stdout).is_empty(),
        "the generated store has attachments"
    );
    let dir = scratch("after-last");
    let resumed = run(&[
        path.as_os_str(),
        "--after".as_ref(),
        last.to_string().as_ref(),
        "--attachments".as_ref(),
        dir.as_os_str(),
    ]);
    assert_eq!(resumed.code, Some(0), "{}", resumed.stderr);
    let lines = resumed.lines();
    assert!(lines.iter().all(|line| owner(line).is_none()));
    let folders = |lines: &[Value]| lines.iter().filter(|line| line["line"] == "folder").count();
    assert_eq!(folders(&lines), folders(&whole.lines()));
    assert!(names(&dir).is_empty());
}

/// An mbox's ordinal counts a separator line with nothing after it: resumed after it, the read
/// gives the messages after it, and its last ordinal is the mbox's last message.
#[test]
fn an_mbox_cursor_counts_an_empty_entry() {
    let mbox = built(
        "after-empty-entry.mbox",
        b"From a@example.com Mon Jan  1 00:00:00 2024\n\
          From: a@example.com\nSubject: one\n\nbody\n\n\
          From b@example.com Mon Jan  1 00:00:00 2024\n\
          From c@example.com Mon Jan  1 00:00:00 2024\n\
          From: c@example.com\nSubject: three\n\nbody\n\n\
          From d@example.com Mon Jan  1 00:00:00 2024\n",
    );
    let whole = run(&[&mbox]);
    for cursor in 1..=4 {
        let out = run(&[
            mbox.as_os_str(),
            "--after".as_ref(),
            cursor.to_string().as_ref(),
        ]);
        assert_eq!(out.code, Some(0), "--after {cursor}: {}", out.stderr);
        assert_eq!(out.stdout, tail_after_ordinal(&whole.stdout, cursor));
    }
}

/// An mbox's tail by ordinal, for one whose empty entries write no line.
fn tail_after_ordinal(stdout: &str, cursor: u64) -> String {
    stdout
        .lines()
        .filter(|raw| {
            let line: Value = serde_json::from_str(raw).unwrap();
            owner(&line).is_none_or(|id| id > cursor)
        })
        .map(|raw| format!("{raw}\n"))
        .collect()
}

/// Asserts a refusal: exit 3, nothing on stdout, and one reason line on stderr.
fn assert_refused(input: &Path, after: &str, reason: &str) {
    let out = run(&[input.as_os_str(), "--after".as_ref(), after.as_ref()]);
    assert_eq!(out.code, Some(3), "{}", out.stderr);
    assert_eq!(out.stdout, "");
    assert_eq!(
        out.stderr,
        format!("spike-ai-pipeline-mail: error: {reason}\n")
    );
}

/// A node id no message in the store has is refused before any output, the node id of a
/// folder included; so is an ordinal past an mbox's last.
#[test]
fn an_id_not_in_the_store_is_refused() {
    let pst = fixture("generated/pst/mailbox.pst");
    let folder = run(&[&pst]).lines()[1]["id"].as_u64().unwrap().to_string();
    for id in ["1", &folder, "4294967295"] {
        assert_refused(
            &pst,
            id,
            &format!(
                "{}: --after {id}: no message in the store has that node id",
                pst.display()
            ),
        );
    }
    let mboxrd = fixture("mime/nine.mboxrd");
    assert_refused(
        &mboxrd,
        "10",
        &format!(
            "{}: --after 10: the mbox's last message is 9",
            mboxrd.display()
        ),
    );
    // A lone separator line is one empty entry: ordinal 1, the store line alone after it.
    let empty = built(
        "after-empty.mbox",
        b"From a@example.com Mon Jan  1 00:00:00 2024\n",
    );
    let out = run(&[empty.as_os_str(), "--after".as_ref(), "1".as_ref()]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(out.stdout, run(&[&empty]).stdout);
    assert_refused(
        &empty,
        "2",
        &format!(
            "{}: --after 2: the mbox's last message is 1",
            empty.display()
        ),
    );
}

/// An id's leading zeros are read: `--after 005` resumes an mbox as `--after 5` does, and
/// `--after 0002097284` a .pst as `--after 2097284` does, byte for byte.
#[test]
fn leading_zeros_are_read() {
    for (input, padded, plain) in [
        ("mime/nine.mboxrd", "005", "5"),
        ("generated/pst/mailbox.pst", "0002097284", "2097284"),
    ] {
        let path = fixture(input);
        let expected = run(&[path.as_os_str(), "--after".as_ref(), plain.as_ref()]);
        assert_eq!(
            expected.code,
            Some(0),
            "{input} --after {plain}: {}",
            expected.stderr
        );
        let out = run(&[path.as_os_str(), "--after".as_ref(), padded.as_ref()]);
        assert_eq!(
            out.code,
            Some(0),
            "{input} --after {padded}: {}",
            out.stderr
        );
        assert!(
            out.stdout == expected.stdout,
            "{input}: --after {padded} is not --after {plain}"
        );
    }
}
