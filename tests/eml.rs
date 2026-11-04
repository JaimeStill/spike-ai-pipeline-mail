//! .eml messages through the binary: the eight generated sources under
//! tests/fixtures/generated/eml against their hand truth (truth.json), and test-built messages
//! for what those don't carry.

mod common;

use common::{built, fixture, run};
use serde_json::{Value, json};

const SOURCES: &[&str] = &[
    "01-plain.eml",
    "02-reply-latin1.eml",
    "03-html-inline-image.eml",
    "04-attachment-cyrillic.eml",
    "05-japanese.eml",
    "06-chinese.eml",
    "07-html-only-bcc.eml",
    "08-forward-attached-message.eml",
];

/// Each truth field, and the field of the model it gives.
const FIELDS: &[(&str, &str)] = &[
    ("subject", "subject"),
    ("sender", "sender"),
    ("date", "submit_time"),
    ("received", "delivery_time"),
    ("message_id", "message_id"),
    ("in_reply_to", "in_reply_to"),
    ("references", "references"),
];

/// A truth recipient list as the model writes it: each recipient with address type SMTP.
fn recipients(truth: Option<&Value>) -> Option<Value> {
    let list = truth?.as_array().unwrap();
    let mapped = list
        .iter()
        .map(|r| {
            let mut r = r.clone();
            r["address_type"] = json!("SMTP");
            r
        })
        .collect();
    Some(Value::Array(mapped))
}

/// The source's header block, its blank line left out, as the transport headers hold it.
fn header_block(bytes: &[u8]) -> String {
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    let end = text.find("\r\n\r\n").unwrap();
    text[..end + 2].to_owned()
}

#[test]
fn eml_headers_match_the_truth() {
    let truth: Value =
        serde_json::from_slice(&std::fs::read(fixture("generated/truth.json")).unwrap()).unwrap();
    for source in SOURCES {
        let path = fixture(&format!("generated/eml/{source}"));
        let out = run(&[&path]);
        assert_eq!(out.code, Some(0), "{source}: {}", out.stderr);
        assert_eq!(out.stderr, "", "{source}");
        let lines = out.lines();
        assert_eq!(lines.len(), 2, "{source}");
        assert_eq!(
            lines[0],
            json!({"line": "store", "schema": "1", "format": "eml"}),
            "{source}"
        );
        let got = &lines[1];
        let truth = &truth["messages"][source];
        assert_eq!(got["line"], "message", "{source}");
        for (from, field) in FIELDS {
            assert_eq!(got.get(field), truth.get(from), "{source}: {field}");
        }
        for field in ["to", "cc", "bcc"] {
            assert_eq!(
                got.get(field),
                recipients(truth.get(field)).as_ref(),
                "{source}: {field}"
            );
        }
        for field in ["id", "folder", "class"] {
            assert_eq!(got.get(field), None, "{source}: {field}");
        }
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            got["transport_headers"].as_str(),
            Some(header_block(&bytes).as_str()),
            "{source}: transport_headers"
        );
    }
}

/// The message line of a test-built .eml, and its stderr.
fn read(name: &str, bytes: &[u8]) -> (Value, String) {
    let out = run(&[built(name, bytes)]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    let lines = out.lines();
    assert_eq!(lines.len(), 2);
    (lines[1].clone(), out.stderr)
}

/// Groups are flattened into their members, in order, and an empty group gives nothing.
#[test]
fn groups_are_flattened() {
    let (message, _) = read(
        "groups.eml",
        b"From: Ann <ann@example.com>, bob@example.com\r\n\
          To: Team: Cy <cy@example.com>, dee@example.com;, eve@example.com\r\n\
          Cc: Nobody:;\r\n\
          \r\n\
          body\r\n",
    );
    assert_eq!(
        message["sender"],
        json!({"name": "Ann", "address": "ann@example.com"})
    );
    assert_eq!(
        message["to"],
        json!([
            {"name": "Cy", "address": "cy@example.com", "address_type": "SMTP"},
            {"address": "dee@example.com", "address_type": "SMTP"},
            {"address": "eve@example.com", "address_type": "SMTP"},
        ])
    );
    assert_eq!(message.get("cc"), None);
}

/// The delivery time is the first Received header's, the newest hop's, in UTC; References is
/// kept as stored, unfolded.
#[test]
fn the_first_received_header_gives_the_delivery_time() {
    let (message, stderr) = read(
        "received.eml",
        b"Received: from b by c; Tue, 13 Jan 2026 01:00:01 +0200\r\n\
          Received: from a by b; Mon, 12 Jan 2026 22:59:59 +0000\r\n\
          Date: Mon, 12 Jan 2026 17:00:00 -0500\r\n\
          References: <a@example.com>\r\n <b@example.com>\r\n\
          \r\n\
          body\r\n",
    );
    assert_eq!(stderr, "");
    assert_eq!(message["delivery_time"], "2026-01-12T23:00:01Z");
    assert_eq!(message["submit_time"], "2026-01-12T22:00:00Z");
    assert_eq!(message["references"], "<a@example.com> <b@example.com>");
}

/// A Date that can't be read is left out, with one warning naming the record "message".
#[test]
fn a_date_that_cannot_be_read_is_left_out_with_a_warning() {
    let (message, stderr) = read(
        "bad-date.eml",
        b"From: ann@example.com\r\nDate: sometime soon\r\n\r\nbody\r\n",
    );
    assert_eq!(message.get("submit_time"), None);
    assert_eq!(
        stderr,
        "spike-ai-pipeline-mail: warning: message: \
         the Date header has no date that can be read; left out\n"
    );
}

/// An .eml is told by its content: a header block whose first field isn't From or Date, and a
/// name that says nothing.
#[test]
fn an_eml_is_sniffed_not_named() {
    let (message, _) = read(
        "message.pst",
        b"Subject: hi\nX-Mailer: test\nDate: Mon, 12 Jan 2026 17:00:00 -0500\n\nbody\n",
    );
    assert_eq!(message["subject"], "hi");
    assert_eq!(message["submit_time"], "2026-01-12T22:00:00Z");
}

/// An .eml of 4 GiB or more is refused before it is read: here a sparse file.
#[test]
fn an_eml_of_4_gib_is_refused() {
    let path = built("huge.eml", b"From: ann@example.com\r\n\r\n");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(1 << 32)
        .unwrap();
    let out = run(&[&path]);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(out.code, Some(3));
    assert_eq!(out.stdout, "");
    assert_eq!(
        out.stderr,
        format!(
            "spike-ai-pipeline-mail: error: {}: an .eml of 4 GiB or more is not read\n",
            path.display()
        )
    );
}
