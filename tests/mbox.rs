//! mbox files through the binary: a store line, then each message in file order, its id its
//! ordinal, with a skipped line in place of a message that can't be read.

mod common;

use common::{built, run};
use serde_json::json;

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
