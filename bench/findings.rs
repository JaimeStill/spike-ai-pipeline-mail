//! The findings run of evidence 16 for the CLI alone: the fields it recovers, per field kind,
//! against the committed fixtures' truth, and its time and peak memory on plaso's store and the
//! scale store. scripts/findings.sh builds it and runs it (`mise run findings`); the check only
//! compiles it. It prints the tables and writes one results file, where the script says.
//!
//! The truth it scores against:
//! - the generated fixtures': tests/fixtures/generated/truth.json, the hand truth of the .eml
//!   sources, for pst/mailbox.pst and each msg/*.msg;
//! - Tika's two stores' threading fields: tests/fixtures/pst/tika-threading.json;
//! - Tika's .msg files': tests/fixtures/msg/tika-truth.json, the values Tika's own tests assert;
//! - Apache POI's .msg files': tests/fixtures/msg/poi-truth.json, the values POI's own HSMF tests
//!   assert.
//!
//! A generated file doesn't hold all of its source: PST Builder and MsgKit each leave fields out
//! or store them otherwise. tests/generated.rs states what each holds and asserts the files hold
//! it; here a value is a generator gap only when the CLI wrote what that header comment says the
//! file holds instead (nothing, or the documented other value), so a gap never hides a CLI fault.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

const USAGE: &str = "usage: findings --bin <cli> --glibc-bin <cli> --rusage <rusage> --plaso <pst> \
--plaso-commit <rev> --scale <dir> --state <dir> --out <results.json> --commit <rev> [--runs <n>]";

/// The arguments scripts/findings.sh passes.
struct Args {
    /// The release binary: static musl, as released.
    bin: PathBuf,
    /// The same CLI built for the host's glibc target, never released: timed beside the release
    /// binary on the two large stores, as musl's allocator is the slower.
    glibc_bin: PathBuf,
    rusage: PathBuf,
    plaso: PathBuf,
    /// The plaso commit the store was fetched at, which scripts/findings.sh pins.
    plaso_commit: String,
    scale: PathBuf,
    state: PathBuf,
    out: PathBuf,
    commit: String,
    runs: usize,
}

fn args() -> Result<Args, String> {
    let mut map = BTreeMap::new();
    let mut args = std::env::args_os().skip(1);
    while let Some(flag) = args.next() {
        let flag = flag.into_string().map_err(|_| USAGE.to_owned())?;
        let value = args.next().ok_or_else(|| USAGE.to_owned())?;
        map.insert(flag, value);
    }
    let runs = match map.remove("--runs") {
        Some(runs) => runs
            .to_str()
            .and_then(|runs| runs.parse().ok())
            .filter(|runs| *runs > 0)
            .ok_or_else(|| USAGE.to_owned())?,
        None => 3,
    };
    let mut take = |flag: &str| map.remove(flag).ok_or_else(|| USAGE.to_owned());
    let args = Args {
        bin: take("--bin")?.into(),
        glibc_bin: take("--glibc-bin")?.into(),
        rusage: take("--rusage")?.into(),
        plaso: take("--plaso")?.into(),
        plaso_commit: take("--plaso-commit")?
            .into_string()
            .map_err(|_| USAGE.to_owned())?,
        scale: take("--scale")?.into(),
        state: take("--state")?.into(),
        out: take("--out")?.into(),
        commit: take("--commit")?
            .into_string()
            .map_err(|_| USAGE.to_owned())?,
        runs,
    };
    if !map.is_empty() {
        return Err(USAGE.to_owned());
    }
    Ok(args)
}

fn main() {
    let args = match args() {
        Ok(args) => args,
        Err(usage) => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    };
    if let Err(err) = run(&args) {
        eprintln!("findings: {err}");
        std::process::exit(1);
    }
}

fn run(args: &Args) -> Result<(), String> {
    let started = now();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures = root.join("tests/fixtures");

    let recovery = json!({
        "generated_pst": generated_pst(&args.bin, &fixtures.join("generated"))?.into_json(),
        "generated_msg": generated_msg(&args.bin, &fixtures.join("generated"))?.into_json(),
        "tika_pst_threading": tika_threading(&args.bin, &fixtures)?.into_json(),
        "tika_msg": tika_msg(&args.bin, &fixtures)?.into_json(),
        "poi_msg": poi_msg(&args.bin, &fixtures)?.into_json(),
    });

    let scale_record = read_json(&args.scale.join("scale.json"))?;
    let floor = measure(args, "floor", Path::new("/bin/true"), None, &[])?;
    let mut timing = Vec::new();
    let mut stores: Vec<(String, PathBuf, usize)> = [
        "generated/pst/mailbox.pst",
        "pst/tika/testPST.pst",
        "pst/tika/testPST_variousBodyTypes.pst",
        "pst/libyal/outlook.pst",
        "pst/java-libpst/dist-list.pst",
        "pst/java-libpst/passworded.pst",
    ]
    .iter()
    .map(|path| (format!("tests/fixtures/{path}"), fixtures.join(path), 1))
    .collect();
    stores.push((
        "plaso nromanoff@stark-research-labs.com.pst".to_owned(),
        args.plaso.clone(),
        args.runs,
    ));
    stores.push((
        "scale.pst".to_owned(),
        args.scale.join("scale.pst"),
        args.runs,
    ));
    for (name, path, runs) in &stores {
        for attachments in [false, true] {
            eprintln!(
                "findings: timing {name}{}",
                if attachments { " --attachments" } else { "" }
            );
            timing.push(time_store(args, &args.bin, name, path, *runs, attachments)?);
        }
    }
    for (name, path, runs) in &stores[stores.len() - 2..] {
        let name = format!("{name} (glibc build, not released)");
        eprintln!("findings: timing {name}");
        timing.push(time_store(
            args,
            &args.glibc_bin,
            &name,
            path,
            *runs,
            false,
        )?);
    }

    let results = json!({
        "schema": 1,
        "started": started,
        "finished": now(),
        "cli": cli(args)?,
        "machine": machine(),
        "recovery": recovery,
        "rusage_floor": floor,
        "plaso": {
            "repository": "https://github.com/log2timeline/plaso",
            "commit": args.plaso_commit,
            "path": "test_data/nromanoff@stark-research-labs.com.pst",
            "license": "Apache-2.0",
            "sha256": sha256(&args.plaso)?,
            "size": size(&args.plaso)?,
        },
        "scale": {
            "size": size(&args.scale.join("scale.pst"))?,
            "record": scale_record,
        },
        "timing": timing,
    });
    print_summary(&results);
    if let Some(dir) = args.out.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(&results).map_err(|e| e.to_string())? + "\n";
    fs::write(&args.out, text).map_err(|e| format!("{}: {e}", args.out.display()))?;
    eprintln!("findings: wrote {}", args.out.display());
    Ok(())
}

// ---- Scoring ------------------------------------------------------------------------------

/// How one expected value fared.
#[derive(Clone, Copy, PartialEq)]
enum Outcome {
    Recovered,
    /// The file holds something else, as tests/generated.rs documents its generator stores it.
    Gap,
    Mismatched,
    Missing,
}

/// The counts of one field kind.
#[derive(Default)]
struct Tally {
    expected: u64,
    recovered: u64,
    gap: u64,
    mismatched: u64,
    missing: u64,
    /// Fields the truth says are absent (Tika's threading nulls), and how many were.
    absent_expected: u64,
    absent_ok: u64,
}

/// The scores of one group of files against one truth.
struct Score {
    files: Vec<String>,
    kinds: BTreeMap<String, Tally>,
    /// Kinds in the order they were first scored, for the tables.
    order: Vec<String>,
    misses: Vec<Value>,
}

impl Score {
    fn new() -> Self {
        Score {
            files: Vec::new(),
            kinds: BTreeMap::new(),
            order: Vec::new(),
            misses: Vec::new(),
        }
    }

    fn tally(&mut self, kind: &str) -> &mut Tally {
        if !self.kinds.contains_key(kind) {
            self.order.push(kind.to_owned());
        }
        self.kinds.entry(kind.to_owned()).or_default()
    }

    /// Records one expected value: `got` against `want`, `gap` saying whether a wrong `got` is
    /// what the generator stores instead.
    fn score(
        &mut self,
        file: &str,
        message: &str,
        kind: &str,
        want: &Value,
        got: Option<&Value>,
        gap: bool,
    ) {
        let outcome = match got {
            Some(got) if got == want => Outcome::Recovered,
            _ if gap => Outcome::Gap,
            Some(_) => Outcome::Mismatched,
            None => Outcome::Missing,
        };
        let tally = self.tally(kind);
        tally.expected += 1;
        match outcome {
            Outcome::Recovered => tally.recovered += 1,
            Outcome::Gap => tally.gap += 1,
            Outcome::Mismatched => tally.mismatched += 1,
            Outcome::Missing => tally.missing += 1,
        }
        if outcome != Outcome::Recovered {
            let name = match outcome {
                Outcome::Gap => "generator gap",
                Outcome::Mismatched => "mismatched",
                _ => "missing",
            };
            self.misses.push(json!({
                "file": file,
                "message": message,
                "kind": kind,
                "outcome": name,
                "expected": clip(want),
                "got": got.map(clip),
            }));
        }
    }

    /// Records a field the truth says is absent.
    fn absent(&mut self, file: &str, message: &str, kind: &str, got: Option<&Value>) {
        let tally = self.tally(kind);
        tally.absent_expected += 1;
        match got {
            None => tally.absent_ok += 1,
            Some(got) => self.misses.push(json!({
                "file": file,
                "message": message,
                "kind": kind,
                "outcome": "unexpected",
                "expected": null,
                "got": clip(got),
            })),
        }
    }

    fn into_json(self) -> Value {
        let kinds: Vec<Value> = self
            .order
            .iter()
            .map(|kind| {
                let t = &self.kinds[kind];
                let mut row = json!({
                    "kind": kind,
                    "expected": t.expected,
                    "recovered": t.recovered,
                    "generator_gap": t.gap,
                    "mismatched": t.mismatched,
                    "missing": t.missing,
                });
                if t.absent_expected > 0 {
                    row["absent_expected"] = json!(t.absent_expected);
                    row["absent_ok"] = json!(t.absent_ok);
                }
                row
            })
            .collect();
        json!({"files": self.files, "kinds": kinds, "misses": self.misses})
    }
}

/// A value shortened for the misses list: a long string keeps its first 160 characters.
fn clip(value: &Value) -> Value {
    match value.as_str() {
        Some(text) if text.chars().count() > 160 => {
            json!(format!("{}...", text.chars().take(160).collect::<String>()))
        }
        _ => value.clone(),
    }
}

/// The CLI's lines for one input, which must exit 0.
fn lines(bin: &Path, input: &Path) -> Result<Vec<Value>, String> {
    let out = Command::new(bin)
        .arg(input)
        .output()
        .map_err(|e| format!("{}: {e}", bin.display()))?;
    if !out.status.success() {
        return Err(format!(
            "{} exited {:?}: {}",
            input.display(),
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    String::from_utf8(out.stdout)
        .map_err(|e| e.to_string())?
        .lines()
        .map(|line| serde_json::from_str(line).map_err(|e| e.to_string()))
        .collect()
}

fn read_json(path: &Path) -> Result<Value, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Line breaks as \n, as the truth writes them.
fn text(value: Option<&Value>) -> Option<Value> {
    Some(json!(value?.as_str()?.replace("\r\n", "\n")))
}

/// An Internet message ID as MsgKit stores it: without its angle brackets.
fn bare(id: &str) -> Value {
    json!(id.trim_start_matches('<').trim_end_matches('>'))
}

/// Which generator made a file, for its gaps.
#[derive(Clone, Copy, PartialEq)]
enum Generator {
    /// PST Builder 1.3.0, through PstBuilder.Eml.
    PstBuilder,
    /// MsgKit 3.0.6.
    MsgKit,
}

/// One generated message's line against its source's truth.
fn score_generated(
    score: &mut Score,
    file: &str,
    source: &str,
    got: &Value,
    truth: &Value,
    generator: Generator,
) {
    let pst = generator == Generator::PstBuilder;
    // A field the truth has, scored; one it hasn't, skipped.
    let field = |score: &mut Score, kind: &str, want: Option<&Value>, got: Option<&Value>, gap| {
        if let Some(want) = want {
            score.score(file, source, kind, want, got, gap);
        }
    };
    field(
        score,
        "subject",
        truth.get("subject"),
        got.get("subject"),
        false,
    );
    if let Some(sender) = truth.get("sender") {
        let read = got
            .get("sender")
            .map(|s| json!({"name": s.get("name"), "address": s.get("address")}));
        field(score, "sender", Some(sender), read.as_ref(), false);
    }
    for list in ["to", "cc", "bcc"] {
        for (i, want) in truth
            .get(list)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            // A recipient without a display name is scored on its address alone: PST Builder
            // names it by its address.
            let read = got.get(list).and_then(|l| l.get(i)).map(|r| {
                let mut read = json!({"address": r.get("address")});
                if want.get("name").is_some() {
                    read["name"] = r.get("name").cloned().unwrap_or(Value::Null);
                }
                read
            });
            let gap = pst && list == "bcc" && read.is_none();
            score.score(
                file,
                source,
                &format!("recipient ({list})"),
                want,
                read.as_ref(),
                gap,
            );
        }
    }
    let date = &truth["date"];
    field(
        score,
        "submit time",
        Some(date),
        got.get("submit_time"),
        false,
    );
    if let Some(received) = truth.get("received") {
        // MsgKit takes the Date header for the delivery time.
        let gap = !pst && got.get("delivery_time") == Some(date);
        field(
            score,
            "delivery time",
            Some(received),
            got.get("delivery_time"),
            gap,
        );
    }
    if let Some(importance) = truth.get("importance") {
        // PST Builder's importance is always normal.
        let gap = pst && got.get("importance") == Some(&json!("normal"));
        field(
            score,
            "importance",
            Some(importance),
            got.get("importance"),
            gap,
        );
    }
    for (kind, key) in [
        ("Message-ID", "message_id"),
        ("In-Reply-To", "in_reply_to"),
        ("References", "references"),
    ] {
        let Some(want) = truth.get(key) else { continue };
        let read = got.get(key);
        let gap = if pst {
            // PST Builder writes no threading fields.
            read.is_none()
        } else {
            // MsgKit stores IDs without their angle brackets, and References as its last ID.
            let ids = want.as_str().unwrap_or_default();
            let stored = if key == "references" {
                bare(ids.split_whitespace().last().unwrap_or_default())
            } else {
                bare(ids)
            };
            read == Some(&stored)
        };
        field(score, kind, Some(want), read, gap);
    }
    for (kind, key) in [("body (plain text)", "text"), ("body (HTML)", "html")] {
        let read = text(got["body"].get(key));
        field(score, kind, truth["body"].get(key), read.as_ref(), false);
    }

    let read: Vec<&Value> = got
        .get("attachments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    for want in truth
        .get("attachments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let sha = want["sha256"].as_str().unwrap_or_default();
        let found = read.iter().find(|a| {
            a.get("file")
                .and_then(Value::as_str)
                .is_some_and(|f| f.split('.').next() == Some(sha))
        });
        let content = found.map(|a| json!({"sha256": sha, "size": a.get("size")}));
        score.score(
            file,
            source,
            "attachment content",
            &json!({"sha256": sha, "size": want["size"]}),
            content.as_ref(),
            false,
        );
        let Some(found) = found else { continue };
        let mut part = |kind: &str, key: &str, gap: bool| {
            if let Some(want) = want.get(key) {
                score.score(file, source, kind, want, found.get(key), gap);
            }
        };
        part("attachment filename", "filename", false);
        // MsgKit types an attachment by its own table of extensions, which has no .csv.
        let msgkit_type = !pst
            && want["filename"]
                .as_str()
                .is_some_and(|name| name.ends_with(".csv"))
            && found.get("mime_type") == Some(&json!("application/octet-stream"));
        part("attachment MIME type", "mime_type", msgkit_type);
        part("attachment content-id", "content_id", false);
        part("attachment inline flag", "inline", false);
    }
    for want in truth
        .get("embedded")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        // Neither generator stores an embedded message: PST Builder skips a message/rfc822
        // part, and MsgKit attaches it as a by-value .eml file named by its subject.
        let nested = read.iter().find_map(|a| a.get("message"));
        let subject = nested.and_then(|m| m.get("subject"));
        let eml = format!("{}.eml", want["subject"].as_str().unwrap_or_default());
        let gap = nested.is_none()
            && (pst
                || read
                    .iter()
                    .any(|a| a.get("filename") == Some(&json!(eml)) && a["method"] == "by_value"));
        score.score(
            file,
            source,
            "embedded message (subject)",
            &want["subject"],
            subject,
            gap,
        );
    }
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| Some(v.as_str()?.to_owned()))
        .collect()
}

/// The generated store against the truth: each message found by its subject, in the folder the
/// layout put it in.
fn generated_pst(bin: &Path, dir: &Path) -> Result<Score, String> {
    let layout = read_json(&dir.join("layout.json"))?;
    let truth = read_json(&dir.join("truth.json"))?;
    let mut score = Score::new();
    for store in layout["stores"].as_array().into_iter().flatten() {
        let file = store["file"].as_str().unwrap_or_default();
        let lines = lines(bin, &dir.join(file))?;
        score.files.push(format!("generated/{file}"));
        let mut paths = BTreeMap::new();
        for line in lines.iter().filter(|l| l["line"] == "folder") {
            // The path under "Top of Personal Folders", the IPM subtree's root.
            let path: Vec<String> = strings(&line["path"]).into_iter().skip(1).collect();
            paths.insert(line["id"].as_u64(), path);
        }
        for folder in store["folders"].as_array().into_iter().flatten() {
            for source in strings(&folder["messages"]) {
                let want = &truth["messages"][&source];
                let got = lines
                    .iter()
                    .find(|l| l["line"] == "message" && l.get("subject") == want.get("subject"));
                let Some(got) = got else {
                    score.score(file, &source, "message", &json!(source), None, false);
                    continue;
                };
                score.score(
                    file,
                    &source,
                    "message",
                    &json!(source),
                    Some(&json!(source)),
                    false,
                );
                let path = paths.get(&got["folder"].as_u64()).map(|p| json!(p));
                score.score(
                    file,
                    &source,
                    "folder",
                    &folder["path"],
                    path.as_ref(),
                    false,
                );
                score_generated(&mut score, file, &source, got, want, Generator::PstBuilder);
            }
        }
    }
    Ok(score)
}

/// Each generated .msg against its source's truth.
fn generated_msg(bin: &Path, dir: &Path) -> Result<Score, String> {
    let layout = read_json(&dir.join("layout.json"))?;
    let truth = read_json(&dir.join("truth.json"))?;
    let mut score = Score::new();
    for source in strings(&layout["msg"]) {
        let file = format!("msg/{}", source.replace(".eml", ".msg"));
        let lines = lines(bin, &dir.join(&file))?;
        score.files.push(format!("generated/{file}"));
        let got = lines.iter().find(|l| l["line"] == "message");
        let Some(got) = got else {
            score.score(&file, &source, "message", &json!(source), None, false);
            continue;
        };
        score.score(
            &file,
            &source,
            "message",
            &json!(source),
            Some(&json!(source)),
            false,
        );
        score_generated(
            &mut score,
            &file,
            &source,
            got,
            &truth["messages"][&source],
            Generator::MsgKit,
        );
    }
    Ok(score)
}

/// Tika's stores' threading fields, message by message in store order; a null is a field the
/// store doesn't hold, scored as an expected absence.
fn tika_threading(bin: &Path, fixtures: &Path) -> Result<Score, String> {
    let truth = read_json(&fixtures.join("pst/tika-threading.json"))?;
    let mut score = Score::new();
    for (store, expected) in truth["stores"].as_object().into_iter().flatten() {
        let lines = lines(bin, &fixtures.join("pst").join(store))?;
        score.files.push(format!("pst/{store}"));
        let messages: Vec<&Value> = lines.iter().filter(|l| l["line"] == "message").collect();
        for (i, want) in expected.as_array().into_iter().flatten().enumerate() {
            let got = messages.get(i);
            let label = want["subject"].as_str().unwrap_or_default();
            for (field, value) in want.as_object().into_iter().flatten() {
                if field == "subject" {
                    continue;
                }
                let read = got.and_then(|m| m.get(field));
                let kind = kind_name(field);
                if value.is_null() {
                    score.absent(store, label, &kind, read);
                } else {
                    score.score(store, label, &kind, value, read, false);
                }
            }
        }
    }
    Ok(score)
}

/// Tika's .msg files against the values Tika's own tests assert, each by its JSON pointer into
/// the message line; the kind is the pointer less its indexes.
fn tika_msg(bin: &Path, fixtures: &Path) -> Result<Score, String> {
    let truth = read_json(&fixtures.join("msg/tika-truth.json"))?;
    let mut score = Score::new();
    for (file, entry) in truth["messages"].as_object().into_iter().flatten() {
        let lines = lines(bin, &fixtures.join("msg").join(file))?;
        score.files.push(format!("msg/{file}"));
        let got = lines.iter().find(|l| l["line"] == "message");
        for (pointer, want) in entry["fields"].as_object().into_iter().flatten() {
            let read = got.and_then(|m| m.pointer(pointer));
            score.score(file, pointer, &pointer_kind(pointer), want, read, false);
        }
    }
    Ok(score)
}

/// Apache POI's .msg files against the values POI's HSMF tests assert, scored as Tika's are:
/// an equal field by its JSON pointer, and, as their own kinds, the Java String.hashCode POI
/// asserts of a body and the length of a recipient list. msgClassAppointment.msg has no
/// assertions; it is read, so it must still exit 0.
fn poi_msg(bin: &Path, fixtures: &Path) -> Result<Score, String> {
    let truth = read_json(&fixtures.join("msg/poi-truth.json"))?;
    let mut score = Score::new();
    for (file, entry) in truth["messages"].as_object().into_iter().flatten() {
        let lines = lines(bin, &fixtures.join("msg").join(file))?;
        score.files.push(format!("msg/{file}"));
        let got = lines.iter().find(|l| l["line"] == "message");
        for (pointer, want) in entry["fields"].as_object().into_iter().flatten() {
            let read = got.and_then(|m| m.pointer(pointer));
            score.score(file, pointer, &pointer_kind(pointer), want, read, false);
        }
        for (pointer, want) in entry["java_hashes"].as_object().into_iter().flatten() {
            let read = got
                .and_then(|m| m.pointer(pointer))
                .and_then(Value::as_str)
                .map(|text| json!(java_hash(text)));
            let kind = format!("{} hash", pointer_kind(pointer));
            score.score(file, pointer, &kind, want, read.as_ref(), false);
        }
        for (pointer, want) in entry["lengths"].as_object().into_iter().flatten() {
            let read = got
                .and_then(|m| m.pointer(pointer))
                .and_then(Value::as_array)
                .map(|list| json!(list.len()));
            let kind = format!("{} count", pointer_kind(pointer));
            score.score(file, pointer, &kind, want, read.as_ref(), false);
        }
    }
    Ok(score)
}

/// Java's String.hashCode, as POI's tests assert it: s[0]*31^(n-1) + ... + s[n-1] over UTF-16
/// code units, wrapping.
fn java_hash(text: &str) -> i32 {
    text.encode_utf16().fold(0i32, |hash, unit| {
        hash.wrapping_mul(31).wrapping_add(i32::from(unit))
    })
}

/// A JSON pointer's field kind: its names less its indexes, as the tables write them.
fn pointer_kind(pointer: &str) -> String {
    pointer
        .split('/')
        .filter(|s| !s.is_empty() && s.parse::<usize>().is_err())
        .map(kind_name)
        .collect::<Vec<_>>()
        .join(" ")
}

/// A field's name as the tables write it.
fn kind_name(field: &str) -> String {
    match field {
        "message_id" => "Message-ID".to_owned(),
        "in_reply_to" => "In-Reply-To".to_owned(),
        "references" => "References".to_owned(),
        other => other.replace('_', " "),
    }
}

// ---- Timing -------------------------------------------------------------------------------

/// One run through the rusage wrapper.
fn measure(
    args: &Args,
    label: &str,
    program: &Path,
    out: Option<(&Path, &Path)>,
    program_args: &[&std::ffi::OsStr],
) -> Result<Value, String> {
    let dir = args.state.join("out");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (stdout, stderr) = match out {
        Some((stdout, stderr)) => (stdout.to_path_buf(), stderr.to_path_buf()),
        None => (
            dir.join(format!("{label}.stdout")),
            dir.join(format!("{label}.stderr")),
        ),
    };
    let result = Command::new(&args.rusage)
        .arg("7200")
        .arg(&stdout)
        .arg(&stderr)
        .arg(program)
        .args(program_args)
        .output()
        .map_err(|e| format!("{}: {e}", args.rusage.display()))?;
    if !result.status.success() {
        return Err(format!(
            "rusage failed on {label}: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    let line = String::from_utf8_lossy(&result.stdout);
    let mut fields = Map::new();
    for pair in line.split_whitespace() {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let value = value.parse::<i64>().map_or(json!(value), |n| json!(n));
        fields.insert(key.to_owned(), value);
    }
    Ok(Value::Object(fields))
}

/// A store's runs, with and without --attachments: each run's time and peak memory, and what the
/// first run wrote.
fn time_store(
    args: &Args,
    bin: &Path,
    name: &str,
    path: &Path,
    runs: usize,
    attachments: bool,
) -> Result<Value, String> {
    let slug: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug = format!("{slug}{}", if attachments { "-attachments" } else { "" });
    let out = args.state.join("out");
    let dir = args.state.join("attachments").join(&slug);
    let mut measured = Vec::new();
    let mut outputs = Vec::new();
    let mut written = json!(null);
    let (stdout, stderr) = (
        out.join(format!("{slug}.jsonl")),
        out.join(format!("{slug}.stderr")),
    );
    for _ in 0..runs {
        // Each run writes into an empty directory, so every content is written.
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let mut cli_args: Vec<&std::ffi::OsStr> = vec![path.as_os_str()];
        if attachments {
            cli_args.push("--attachments".as_ref());
            cli_args.push(dir.as_os_str());
        }
        let run = measure(args, &slug, bin, Some((&stdout, &stderr)), &cli_args)?;
        if run["status"] != "exited" || run["code"] != 0 {
            return Err(format!("{name}: {run}"));
        }
        measured.push(run);
        outputs.push(sha256(&stdout)?);
        if attachments {
            written = files_in(&dir)?;
        }
    }
    let mut kinds: BTreeMap<String, u64> = BTreeMap::new();
    let text = fs::read_to_string(&stdout).map_err(|e| format!("{}: {e}", stdout.display()))?;
    for line in text.lines() {
        let kind = line
            .strip_prefix("{\"line\":\"")
            .and_then(|rest| rest.split('"').next())
            .unwrap_or("?");
        *kinds.entry(kind.to_owned()).or_default() += 1;
    }
    let attachments_listed = text.matches("\"method\":").count();
    let warnings = fs::read_to_string(&stderr)
        .map_err(|e| format!("{}: {e}", stderr.display()))?
        .lines()
        .count();
    let mut walls: Vec<i64> = measured
        .iter()
        .filter_map(|r| r["wall_ns"].as_i64())
        .collect();
    walls.sort_unstable();
    let peak = measured
        .iter()
        .filter_map(|r| r["maxrss_kib"].as_i64())
        .max();
    Ok(json!({
        "store": name,
        "size": size(path)?,
        "attachments": attachments,
        "runs": measured,
        "median_wall_ns": walls[walls.len() / 2],
        "peak_maxrss_kib": peak,
        "stdout_bytes": text.len(),
        "same_stdout_every_run": outputs.windows(2).all(|w| w[0] == w[1]),
        "lines": kinds,
        "attachment_records": attachments_listed,
        "warnings": warnings,
        "written": written,
    }))
}

/// The files a run wrote into its attachments directory, and their bytes.
fn files_in(dir: &Path) -> Result<Value, String> {
    let (mut files, mut bytes) = (0u64, 0u64);
    if dir.exists() {
        for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let entry = entry.map_err(|e| e.to_string())?;
            files += 1;
            bytes += entry.metadata().map_err(|e| e.to_string())?.len();
        }
    }
    Ok(json!({"files": files, "bytes": bytes}))
}

// ---- The run's facts ----------------------------------------------------------------------

fn sha256(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let mut file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn size(path: &Path) -> Result<u64, String> {
    Ok(fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len())
}

fn cli(args: &Args) -> Result<Value, String> {
    let version = Command::new(&args.bin)
        .arg("--version")
        .output()
        .map_err(|e| format!("{}: {e}", args.bin.display()))?;
    Ok(json!({
        "version": String::from_utf8_lossy(&version.stdout).trim(),
        "commit": args.commit,
        "binary": args.bin.file_name().map(|n| n.to_string_lossy()),
        "build": "release, static musl, the check's target",
        "sha256": sha256(&args.bin)?,
        "size": size(&args.bin)?,
    }))
}

/// The CPU, its logical CPUs, the memory and the kernel, from /proc.
fn machine() -> Value {
    let read = |path: &str| fs::read_to_string(path).unwrap_or_default();
    let field = |text: &str, key: &str| {
        text.lines()
            .find(|line| line.starts_with(key))
            .and_then(|line| line.split_once(':'))
            .map(|(_, value)| value.trim().to_owned())
    };
    let cpuinfo = read("/proc/cpuinfo");
    let meminfo = read("/proc/meminfo");
    json!({
        "cpu": field(&cpuinfo, "model name"),
        "logical_cpus": std::thread::available_parallelism().map(|n| n.get()).ok(),
        "memory": field(&meminfo, "MemTotal"),
        "kernel": read("/proc/sys/kernel/osrelease").trim(),
    })
}

/// The time now, ISO 8601 UTC to the second.
fn now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

// ---- The summary --------------------------------------------------------------------------

/// The tables, as Markdown, to stdout.
fn print_summary(results: &Value) {
    println!(
        "spike-ai-pipeline-mail {} ({})",
        results["cli"]["version"].as_str().unwrap_or("?"),
        results["cli"]["commit"].as_str().unwrap_or("?")
    );
    for (group, title) in [
        ("generated_pst", "Generated .pst against truth.json"),
        ("generated_msg", "Generated .msg against truth.json"),
        (
            "tika_pst_threading",
            "Tika's stores against the threading truth",
        ),
        ("tika_msg", "Tika's .msg against Tika's assertions"),
        ("poi_msg", "Apache POI's .msg against POI's assertions"),
    ] {
        let score = &results["recovery"][group];
        println!("\n### {title}\n");
        println!("Files: {}\n", strings(&score["files"]).join(", "));
        println!(
            "| Field kind | Expected | Recovered | Generator gap | Mismatched | Missing | Absent, as the truth says |"
        );
        println!("|---|---|---|---|---|---|---|");
        for row in score["kinds"].as_array().into_iter().flatten() {
            let absent = match (row.get("absent_ok"), row.get("absent_expected")) {
                (Some(ok), Some(of)) => format!("{ok} / {of}"),
                _ => "".to_owned(),
            };
            println!(
                "| {} | {} | {} | {} | {} | {} | {absent} |",
                row["kind"].as_str().unwrap_or("?"),
                row["expected"],
                row["recovered"],
                row["generator_gap"],
                row["mismatched"],
                row["missing"],
            );
        }
        let faults: Vec<&Value> = score["misses"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["outcome"] != "generator gap")
            .collect();
        for miss in faults {
            println!(
                "- {} {} {}: {}, expected {}, got {}",
                miss["file"].as_str().unwrap_or("?"),
                miss["message"].as_str().unwrap_or("?"),
                miss["kind"].as_str().unwrap_or("?"),
                miss["outcome"].as_str().unwrap_or("?"),
                miss["expected"],
                miss["got"]
            );
        }
    }
    println!("\n### Time and peak memory\n");
    println!(
        "rusage floor (/bin/true): {} KiB, {:.2} ms\n",
        results["rusage_floor"]["maxrss_kib"],
        results["rusage_floor"]["wall_ns"].as_f64().unwrap_or(0.0) / 1e6
    );
    println!(
        "| Store | Size | --attachments | Runs | Median wall | Peak memory | Messages | Folders | Skipped | Warnings | Files written |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    for row in results["timing"].as_array().into_iter().flatten() {
        let written = if row["attachments"] == true {
            format!(
                "{} ({} bytes)",
                row["written"]["files"], row["written"]["bytes"]
            )
        } else {
            "".to_owned()
        };
        println!(
            "| {} | {} | {} | {} | {} | {:.1} MiB | {} | {} | {} | {} | {written} |",
            row["store"].as_str().unwrap_or("?"),
            row["size"],
            if row["attachments"] == true {
                "yes"
            } else {
                "no"
            },
            row["runs"].as_array().map_or(0, Vec::len),
            duration(row["median_wall_ns"].as_f64().unwrap_or(0.0)),
            row["peak_maxrss_kib"].as_f64().unwrap_or(0.0) / 1024.0,
            row["lines"].get("message").unwrap_or(&json!(0)),
            row["lines"].get("folder").unwrap_or(&json!(0)),
            row["lines"].get("skipped").unwrap_or(&json!(0)),
            row["warnings"],
        );
    }
}

/// A wall time in nanoseconds, in milliseconds under 10 s, else in seconds.
fn duration(ns: f64) -> String {
    if ns < 1e10 {
        format!("{:.1} ms", ns / 1e6)
    } else {
        format!("{:.2} s", ns / 1e9)
    }
}
