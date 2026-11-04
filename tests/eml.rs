//! .eml messages through the binary: the eight generated sources under
//! tests/fixtures/generated/eml against their hand truth (truth.json), the MIME group's
//! hand-written sources under tests/fixtures/mime/eml against theirs, and test-built messages
//! for what those don't carry.

mod common;

use common::{built, fixture, names, run, run_in, scratch, sha256};
use serde_json::{Value, json};

/// The MIME group's hand-written sources, under tests/fixtures/mime/eml.
const MIME_SOURCES: &[&str] = &["09-threading.eml"];

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

/// Each truth field compared as it is, and the field of the model it gives.
const FIELDS: &[(&str, &str)] = &[
    ("subject", "subject"),
    ("date", "submit_time"),
    ("received", "delivery_time"),
    ("message_id", "message_id"),
    ("in_reply_to", "in_reply_to"),
    ("references", "references"),
    ("importance", "importance"),
    ("sensitivity", "sensitivity"),
    ("conversation_topic", "conversation_topic"),
    ("conversation_index", "conversation_index"),
];

/// The truth fields compared otherwise: each address with address type SMTP, the bodies, the
/// attachments and embedded messages as attachment records. The charset is the source's label,
/// no field of the model.
const MAPPED: &[&str] = &[
    "sender",
    "to",
    "cc",
    "bcc",
    "body",
    "attachments",
    "embedded",
    "charset",
];

/// A group's truth, by its directory under tests/fixtures.
fn truth(group: &str) -> Value {
    serde_json::from_slice(&std::fs::read(fixture(&format!("{group}/truth.json"))).unwrap())
        .unwrap()
}

/// A truth address as the model writes it: with address type SMTP.
fn smtp(address: &Value) -> Value {
    let mut address = address.clone();
    address["address_type"] = json!("SMTP");
    address
}

/// A truth recipient list as the model writes it.
fn recipients(truth: Option<&Value>) -> Option<Value> {
    let list = truth?.as_array().unwrap();
    Some(Value::Array(list.iter().map(smtp).collect()))
}

/// The attachment records a truth entry gives, in the source's order: each attachment by value,
/// its file named by its SHA-256 and its filename's extension, then each attached message,
/// embedded, without a file. The generated sources attach their messages last.
fn attachments(truth: &Value) -> Vec<Value> {
    let mut records: Vec<Value> = truth
        .get("attachments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
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
    let embedded = truth.get("embedded").and_then(Value::as_array);
    records.extend(embedded.into_iter().flatten().map(|_| {
        json!({
            "mime_type": "message/rfc822",
            "inline": false,
            "method": "embedded_message",
        })
    }));
    records
}

/// The source's header block, its blank line left out, as the transport headers hold it.
fn header_block(bytes: &[u8]) -> String {
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    let end = text.find("\r\n\r\n").unwrap();
    text[..end + 2].to_owned()
}

/// One message against its truth entry: a message line, or, with `nested`, a message embedded
/// in another, which is no line. The transport headers are the caller's to check.
fn assert_message(got: &Value, truth: &Value, source: &str, nested: bool) {
    for key in truth.as_object().unwrap().keys() {
        assert!(
            MAPPED.contains(&key.as_str()) || FIELDS.iter().any(|(from, _)| from == key),
            "{source}: truth field {key} is compared"
        );
    }
    assert_eq!(
        got.get("line"),
        (!nested).then(|| json!("message")).as_ref(),
        "{source}: line"
    );
    for (from, field) in FIELDS {
        assert_eq!(got.get(field), truth.get(from), "{source}: {field}");
    }
    assert_eq!(
        got.get("sender"),
        truth.get("sender").map(smtp).as_ref(),
        "{source}: sender"
    );
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

    let expected = attachments(truth);
    let records = got.get("attachments").and_then(Value::as_array);
    let records = records.map(Vec::as_slice).unwrap_or_default();
    assert_eq!(records.len(), expected.len(), "{source}: attachments");
    let embedded = truth.get("embedded").and_then(Value::as_array);
    let mut embedded = embedded.into_iter().flatten();
    for (record, expected) in records.iter().zip(expected) {
        let mut record = record.clone();
        let message = record.as_object_mut().unwrap().remove("message");
        assert_eq!(record, expected, "{source}: attachment");
        if record["method"] == "embedded_message" {
            let truth = embedded.next().unwrap();
            let message = message.expect("an embedded message is nested under its record");
            let id = truth["message_id"].as_str().unwrap();
            assert!(
                message["transport_headers"]
                    .as_str()
                    .unwrap()
                    .starts_with(&format!("Message-ID: {id}\r\n")),
                "{source}: embedded transport headers"
            );
            assert_message(&message, truth, &format!("{source}, embedded"), true);
        } else {
            assert_eq!(message, None, "{source}: attachment message");
        }
    }
}

/// Each .eml of a group is a store line and one message line that matches the group's truth in
/// full, without a warning.
fn assert_group_matches_its_truth(group: &str, sources: &[&str]) {
    let truth = truth(group);
    let listed: Vec<&String> = truth["messages"].as_object().unwrap().keys().collect();
    assert_eq!(listed, sources, "{group}: every source has its truth");
    for source in sources {
        let path = fixture(&format!("{group}/eml/{source}"));
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
        assert_message(&lines[1], &truth["messages"][source], source, false);
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            lines[1]["transport_headers"].as_str(),
            Some(header_block(&bytes).as_str()),
            "{source}: transport_headers"
        );
    }
}

/// Each generated .eml matches truth.json in full.
#[test]
fn eml_files_match_the_truth() {
    assert_group_matches_its_truth("generated", SOURCES);
}

/// The MIME group's hand-written .eml matches its own truth in full: the threading and
/// priority headers, and the body lines starting "From " and ">From " as they are.
#[test]
fn the_mime_group_matches_its_truth() {
    assert_group_matches_its_truth("mime", MIME_SOURCES);
}

/// Every file of the MIME group has the SHA-256 its list records, and every committed file is
/// listed, but the list itself.
#[test]
fn the_mime_group_matches_its_sha256_list() {
    let list: Value =
        serde_json::from_slice(&std::fs::read(fixture("mime/sha256.json")).unwrap()).unwrap();
    let mut listed = Vec::new();
    for entry in list.as_array().unwrap() {
        let file = entry["file"].as_str().unwrap();
        assert_eq!(
            Some(sha256(&fixture(&format!("mime/{file}"))).as_str()),
            entry["sha256"].as_str(),
            "{file}"
        );
        listed.push(file.to_owned());
    }
    listed.sort_unstable();
    let mut committed = Vec::new();
    let mut dirs = vec![String::new()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(fixture(&format!("mime/{dir}"))).unwrap() {
            let entry = entry.unwrap();
            let name = format!("{dir}{}", entry.file_name().into_string().unwrap());
            if entry.file_type().unwrap().is_dir() {
                dirs.push(format!("{name}/"));
            } else if name != "sha256.json" {
                committed.push(name);
            }
        }
    }
    committed.sort_unstable();
    assert_eq!(listed, committed, "the list names every file of the group");
}

/// The Markdown body is made from the HTML, keeping its cid: image, else from the text.
#[test]
fn markdown_is_made_from_the_html_else_the_text() {
    let markdown = |source: &str| {
        let out = run(&[fixture(&format!("generated/eml/{source}"))]);
        out.lines()[1]["body"]["derived"]["markdown"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(
        markdown("03-html-inline-image.eml"),
        "We found a **moth** in relay 70, panel F.\n\n![moth](cid:moth@example.net)"
    );
    assert_eq!(
        markdown("02-reply-latin1.eml"),
        "Ada,\n\nSplendid. The café in Zürich serves crème brûlée; we shall  \n\
         discuss Note G there.\n\n\\> I have finished the notes on the engine.\n\nCharles"
    );
}

/// With --attachments, each distinct content is written once, named by its SHA-256 and
/// extension, in a directory created as it was missing; an embedded message has no file.
#[test]
fn attachments_are_written_by_content() {
    for source in SOURCES {
        let dir = scratch(&format!("eml-attachments-{source}")).join("created");
        let path = fixture(&format!("generated/eml/{source}"));
        let out = run(&[path.as_os_str(), "--attachments".as_ref(), dir.as_os_str()]);
        assert_eq!(out.code, Some(0), "{source}: {}", out.stderr);
        let lines = out.lines();
        let mut expected: Vec<String> = lines[1]
            .get("attachments")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|record| Some(record.get("file")?.as_str()?.to_owned()))
            .collect();
        expected.sort_unstable();
        expected.dedup();
        assert_eq!(names(&dir), expected, "{source}");
        for name in &expected {
            let (sha, _) = name.split_once('.').unwrap();
            assert_eq!(sha256(&dir.join(name)), sha, "{source}: {name}");
        }
    }
}

/// Without --attachments, a run writes nothing to disk: its working directory stays empty.
#[test]
fn nothing_is_written_without_the_flag() {
    let dir = scratch("eml-no-attachments");
    let out = run_in(&dir, &[fixture("generated/eml/03-html-inline-image.eml")]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(names(&dir), Vec::<String>::new());
}

/// The message line of a test-built .eml, and its stderr.
fn read(name: &str, bytes: &[u8]) -> (Value, String) {
    let out = run(&[built(name, bytes)]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    let lines = out.lines();
    assert_eq!(lines.len(), 2);
    (lines[1].clone(), out.stderr)
}

/// A test-built .eml of these header lines and a one-line body.
fn with_headers(name: &str, headers: &str) -> (Value, String) {
    read(
        name,
        format!("From: ann@example.com\r\n{headers}\r\nbody\r\n").as_bytes(),
    )
}

/// The warning line for a record's note.
fn warning(note: &str) -> String {
    format!("spike-ai-pipeline-mail: warning: message: {note}\n")
}

/// Importance gives the importance, whatever its case, before X-Priority; without it,
/// X-Priority's leading number gives its band.
#[test]
fn importance_comes_from_importance_else_x_priority() {
    let cases = [
        ("Importance: Low\r\nX-Priority: 1\r\n", "low"),
        (
            "X-Priority: 1 (Highest)\r\nImportance: normal\r\n",
            "normal",
        ),
        ("Importance: HIGH\r\n", "high"),
        ("X-Priority: 1 (Highest)\r\n", "high"),
        ("X-Priority: 2\r\n", "high"),
        ("X-Priority: 3 (Normal)\r\n", "normal"),
        ("X-Priority: 4\r\n", "low"),
        ("X-Priority: 5 (Lowest)\r\n", "low"),
    ];
    for (headers, importance) in cases {
        let (message, stderr) = with_headers("importance.eml", headers);
        assert_eq!(message["importance"], importance, "{headers:?}");
        assert_eq!(stderr, "", "{headers:?}");
    }
}

/// Sensitivity's three values, whatever their case, in schema "1"'s words.
#[test]
fn sensitivity_values_map_to_the_schemas() {
    let cases = [
        ("Personal", "personal"),
        ("private", "private"),
        ("Company-Confidential", "confidential"),
    ];
    for (value, sensitivity) in cases {
        let (message, stderr) =
            with_headers("sensitivity.eml", &format!("Sensitivity: {value}\r\n"));
        assert_eq!(message["sensitivity"], sensitivity, "{value}");
        assert_eq!(stderr, "", "{value}");
    }
}

/// A value that maps to none of the model's is left out, with a warning naming the header; an
/// unknown Importance falls back to X-Priority.
#[test]
fn unknown_values_are_left_out_with_a_warning() {
    let cases = [
        (
            "Importance: urgent\r\n",
            None,
            "the Importance header has unknown value \"urgent\"; left out",
        ),
        (
            "Importance: urgent\r\nX-Priority: 5\r\n",
            Some("low"),
            "the Importance header has unknown value \"urgent\"; left out",
        ),
        (
            "X-Priority: 6\r\n",
            None,
            "the X-Priority header has unknown value \"6\"; left out",
        ),
        (
            "X-Priority: Highest\r\n",
            None,
            "the X-Priority header has unknown value \"Highest\"; left out",
        ),
        (
            "X-Priority: 10\r\n",
            None,
            "the X-Priority header has unknown value \"10\"; left out",
        ),
    ];
    for (headers, importance, note) in cases {
        let (message, stderr) = with_headers("unknown.eml", headers);
        assert_eq!(
            message.get("importance").and_then(Value::as_str),
            importance,
            "{headers:?}"
        );
        assert_eq!(stderr, warning(note), "{headers:?}");
    }
    let (message, stderr) = with_headers("unknown.eml", "Sensitivity: Normal\r\n");
    assert_eq!(message.get("sensitivity"), None);
    assert_eq!(
        stderr,
        warning("the Sensitivity header has unknown value \"Normal\"; left out")
    );
}

/// Thread-Index is base64-decoded into the conversation index, in lowercase hex; one that
/// isn't valid base64 is left out, with a warning. Thread-Topic is decoded from its encoded
/// words.
#[test]
fn thread_index_and_topic_fill_the_conversation_fields() {
    let (message, stderr) = with_headers(
        "thread.eml",
        "Thread-Index: AQL/\r\nThread-Topic: =?ISO-8859-1?Q?caf=E9?=\r\n",
    );
    assert_eq!(stderr, "");
    assert_eq!(message["conversation_index"], "0102ff");
    assert_eq!(message["conversation_topic"], "café");
    let (message, stderr) = with_headers("thread.eml", "Thread-Index: not base64!\r\n");
    assert_eq!(message.get("conversation_index"), None);
    assert_eq!(
        stderr,
        warning("the Thread-Index header is not valid base64; left out")
    );
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
        json!({"name": "Ann", "address": "ann@example.com", "address_type": "SMTP"})
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

/// A message `levels` deep: each level a text part and, but for the last, the next level
/// attached as message/rfc822, so the last is `levels - 1` messages below the top.
fn nested(levels: usize) -> Vec<u8> {
    let mut message = format!(
        "From: ann@example.com\r\nSubject: level {}\r\n\r\nthe last level\r\n",
        levels - 1
    );
    for level in (0..levels - 1).rev() {
        message = format!(
            "From: ann@example.com\r\nSubject: level {level}\r\n\
             Content-Type: multipart/mixed; boundary=\"b{level}\"\r\n\r\n\
             --b{level}\r\nContent-Type: text/plain\r\n\r\nlevel {level}\r\n\
             --b{level}\r\nContent-Type: message/rfc822\r\n\r\n{message}\r\n\
             --b{level}--\r\n"
        );
    }
    message.into_bytes()
}

/// Embedded messages nest to 16 levels below the top; one deeper is listed, embedded, without
/// its message, with one warning naming its place.
#[test]
fn nesting_stops_at_16_levels() {
    let (message, stderr) = read("nested.eml", &nested(18));
    let mut at = &message;
    let mut place = "message".to_owned();
    for level in 1..=16 {
        let record = &at["attachments"][0];
        assert_eq!(record["method"], "embedded_message", "level {level}");
        at = &record["message"];
        assert_eq!(at["subject"], format!("level {level}"));
        assert_eq!(at.get("line"), None, "level {level}");
        place.push_str(", attachment 1, message");
    }
    let record = &at["attachments"][0];
    assert_eq!(
        record,
        &json!({"mime_type": "message/rfc822", "inline": false, "method": "embedded_message"})
    );
    assert_eq!(
        stderr,
        format!(
            "spike-ai-pipeline-mail: warning: {place}, attachment 1: \
             an embedded message nested deeper than 16 messages; listed without it\n"
        )
    );
}

/// `bytes` in base64, by coreutils' base64, so the tests take no encoding crate.
fn base64(bytes: &[u8]) -> String {
    let path = built("base64.in", bytes);
    let out = std::process::Command::new("base64")
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap().replace('\n', "\r\n")
}

/// A transfer-encoded message/rfc822 part is an embedded message as an unencoded one is, at
/// every level, past mail-parser's default of 3 encoded levels; an inline disposition is kept.
#[test]
fn an_encoded_message_is_embedded() {
    let mut message = "From: ann@example.com\r\nSubject: level 5\r\n\r\nlast\r\n".to_owned();
    for level in (0..5).rev() {
        message = format!(
            "From: ann@example.com\r\nSubject: level {level}\r\n\
             Content-Type: message/rfc822\r\nContent-Disposition: inline\r\n\
             Content-Transfer-Encoding: base64\r\n\r\n{}",
            base64(message.as_bytes())
        );
    }
    let (message, stderr) = read("encoded.eml", message.as_bytes());
    assert_eq!(stderr, "");
    let mut at = &message;
    for level in 1..=5 {
        let record = &at["attachments"][0];
        assert_eq!(record["method"], "embedded_message", "level {level}");
        assert_eq!(record["inline"], true, "level {level}");
        assert_eq!(record.get("file"), None, "level {level}");
        at = &record["message"];
        assert_eq!(at["subject"], format!("level {level}"));
    }
    assert_eq!(at["body"]["text"], "last\r\n");
}

/// The text body is the text parts joined in order, an attachment between them; each
/// attachment part is a record in order, its filename, type and content ID from the part.
#[test]
fn text_parts_are_joined_and_attachments_listed_in_order() {
    let dir = scratch("eml-ordered-attachments");
    let path = built(
        "ordered.eml",
        b"From: ann@example.com\r\n\
          Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n\
          --b\r\nContent-Type: text/plain\r\n\r\none\r\n\
          --b\r\nContent-Type: application/octet-stream; name=\"a.bin\"\r\n\
          Content-ID: <a@example.com>\r\nContent-Transfer-Encoding: base64\r\n\r\nAAEC\r\n\
          --b\r\nContent-Type: text/plain\r\n\r\ntwo\r\n\
          --b\r\nContent-Type: application/octet-stream\r\n\
          Content-Disposition: attachment; filename=\"same\"\r\n\
          Content-Transfer-Encoding: base64\r\n\r\nAAEC\r\n\
          --b--\r\n",
    );
    let out = run(&[path.as_os_str(), "--attachments".as_ref(), dir.as_os_str()]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(out.stderr, "");
    let message = &out.lines()[1];
    assert_eq!(message["body"]["text"], "one\ntwo");
    let sha = "ae4b3280e56e2faf83f414a6e3dabe9d5fbe18976544c05fed121accb85b53fc";
    assert_eq!(
        message["attachments"],
        json!([
            {"file": format!("{sha}.bin"), "filename": "a.bin",
             "mime_type": "application/octet-stream", "content_id": "a@example.com",
             "inline": false, "method": "by_value", "size": 3},
            {"file": sha, "filename": "same", "mime_type": "application/octet-stream",
             "inline": false, "method": "by_value", "size": 3},
        ])
    );
    assert_eq!(names(&dir), [sha.to_owned(), format!("{sha}.bin")]);
}

/// US-ASCII and ISO-8859-1, in parts and in encoded words, decode as windows-1252.
#[test]
fn latin1_and_ascii_decode_as_windows_1252() {
    let (message, stderr) = read(
        "latin1.eml",
        b"From: ann@example.com\r\n\
          Subject: =?US-ASCII?Q?=805?= =?ISO-8859-1?Q?caf=E9?=\r\n\
          Content-Type: multipart/alternative; boundary=\"b\"\r\n\r\n\
          --b\r\nContent-Type: text/plain; charset=ISO-8859-1\r\n\
          Content-Transfer-Encoding: quoted-printable\r\n\r\ncaf=E9 =80 =9F\r\n\
          --b\r\nContent-Type: text/html; charset=us-ascii\r\n\r\n<p>\x93caf\xe9\x94</p>\r\n\
          --b--\r\n",
    );
    assert_eq!(stderr, "");
    assert_eq!(message["subject"], "€5café");
    assert_eq!(message["body"]["text"], "café € Ÿ");
    assert_eq!(message["body"]["html"], "<p>\u{201c}café\u{201d}</p>");
}

/// A byte windows-1252 leaves undefined becomes U+FFFD, and malformed base64 is decoded as
/// best it can be, each with one warning naming its record. An attachment's content is its
/// bytes, so its charset, here an unknown one, is never decoded.
#[test]
fn lossy_decoding_gives_a_warning_naming_the_record() {
    let (message, stderr) = read(
        "lossy.eml",
        b"From: ann@example.com\r\n\
          Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n\
          --b\r\nContent-Type: text/plain; charset=iso-8859-1\r\n\r\nA\x81B\x8d\r\n\
          --b\r\nContent-Type: application/octet-stream\r\n\
          Content-Transfer-Encoding: base64\r\n\r\nAA*EC\r\n\
          --b\r\nContent-Type: text/plain; charset=x-unknown\r\n\
          Content-Disposition: attachment\r\n\r\nplain\r\n\
          --b--\r\n",
    );
    assert_eq!(message["body"]["text"], "A\u{fffd}B\u{fffd}");
    assert_eq!(message["attachments"][0]["size"], 3);
    assert_eq!(
        stderr,
        "spike-ai-pipeline-mail: warning: message: \
         the text body is not valid in its charset; decoded lossily\n\
         spike-ai-pipeline-mail: warning: message, attachment 1: \
         the attachment's content has a malformed transfer encoding; decoded lossily\n"
    );
}
