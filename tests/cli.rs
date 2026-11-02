//! The command line: arguments, usage errors, and the inputs refused with exit 3.

mod common;

use common::{built, fixture, run};

const USAGE: &str = "usage: spike-ai-pipeline-mail [--attachments <dir>] <input>
       spike-ai-pipeline-mail --version
";

#[test]
fn version_prints_the_crate_version() {
    let out = run(&["--version"]);
    assert_eq!(out.code, Some(0));
    assert_eq!(
        out.stdout,
        format!("spike-ai-pipeline-mail {}\n", env!("CARGO_PKG_VERSION"))
    );
}

/// Each usage error exits 2 with its reason and the usage on stderr, and nothing on stdout.
#[test]
fn usage_errors_exit_2_with_usage() {
    let store = fixture("pst/libyal/outlook.pst");
    let store = store.to_str().unwrap();
    let not_a_dir = fixture("pst/sources.json");
    let not_a_dir = not_a_dir.to_str().unwrap();
    let cases: &[(&[&str], &str)] = &[
        (&[], "no input"),
        (&[store, store], "more than one input"),
        (&["--frobnicate", store], "unknown option --frobnicate"),
        (&["--version", store], "--version takes no other arguments"),
        (&[store, "--attachments"], "--attachments needs a directory"),
        (
            &["--attachments", "", store],
            "--attachments needs a directory",
        ),
        (
            &["--attachments", "a", "--attachments", "b", store],
            "--attachments is given more than once",
        ),
        (
            &["--attachments", not_a_dir, store],
            &format!("--attachments {not_a_dir} is not a directory"),
        ),
    ];
    for (args, reason) in cases {
        let out = run(args);
        assert_eq!(out.code, Some(2), "{args:?}");
        assert_eq!(out.stdout, "", "{args:?}");
        assert_eq!(
            out.stderr,
            format!("spike-ai-pipeline-mail: {reason}\n{USAGE}"),
            "{args:?}"
        );
    }
}

/// --attachments is accepted before or after the input, and a directory that doesn't exist yet
/// is accepted: it is created when attachments are written.
#[test]
fn attachments_is_accepted_with_the_input() {
    let store = fixture("pst/libyal/outlook.pst");
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("attachments-unmade");
    let without = run(&[&store]);
    for args in [
        [store.as_os_str(), "--attachments".as_ref(), dir.as_os_str()],
        ["--attachments".as_ref(), dir.as_os_str(), store.as_os_str()],
    ] {
        let out = run(&args);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, without.stdout);
    }
}

/// A PST header with the given client magic and version, padded to the header's size, with
/// nothing valid after its first 12 bytes.
fn pst_header(client: &[u8; 2], version: u16) -> Vec<u8> {
    let mut bytes = b"!BDN\0\0\0\0".to_vec();
    bytes.extend_from_slice(client);
    bytes.extend_from_slice(&version.to_le_bytes());
    bytes.resize(564, 0);
    bytes
}

/// Asserts a refusal: exit 3, nothing on stdout, and one reason line on stderr.
fn assert_refused(args: &[&std::path::Path], reason: &str) {
    let out = run(args);
    assert_eq!(out.code, Some(3), "{}", out.stderr);
    assert_eq!(out.stdout, "");
    assert_eq!(
        out.stderr,
        format!("spike-ai-pipeline-mail: error: {reason}\n")
    );
}

#[test]
fn an_ost_is_refused_with_how_to_export_it() {
    let ost = built("refused.ost", &pst_header(b"SO", 36));
    assert_refused(
        &[&ost],
        &format!(
            "{}: an .ost, the offline cache of a server mailbox, is not read; \
             export the mailbox from Outlook as a .pst",
            ost.display()
        ),
    );
}

/// The format is sniffed, never taken from the name: an .ost header named .pst is still an .ost.
#[test]
fn the_format_is_sniffed_not_named() {
    let ost = built("named.pst", &pst_header(b"SO", 23));
    let out = run(&[&ost]);
    assert_eq!(out.code, Some(3));
    assert!(out.stderr.contains("an .ost"), "{}", out.stderr);
}

#[test]
fn a_pst_version_outlook_pst_does_not_read_is_refused() {
    let pst = built("4k-pages.pst", &pst_header(b"SM", 36));
    assert_refused(
        &[&pst],
        &format!(
            "{}: PST version 36 is not read; \
             only versions 14 and 15 (ANSI) and 23 (Unicode) are",
            pst.display()
        ),
    );
}

#[test]
fn an_unreadable_pst_header_is_refused() {
    let pst = built("bad-header.pst", &pst_header(b"SM", 23));
    let out = run(&[&pst]);
    assert_eq!(out.code, Some(3));
    assert_eq!(out.stdout, "");
    assert!(
        out.stderr
            .starts_with("spike-ai-pipeline-mail: error: the PST header can't be read: "),
        "{}",
        out.stderr
    );
    assert_eq!(out.stderr.lines().count(), 1);
}

/// A store cut short after its header: the header reads, the store's root doesn't.
#[test]
fn an_unreadable_store_root_is_refused() {
    let whole = std::fs::read(fixture("pst/libyal/outlook.pst")).unwrap();
    let pst = built("truncated.pst", &whole[..4096]);
    let out = run(&[&pst]);
    assert_eq!(out.code, Some(3));
    assert_eq!(out.stdout, "");
    assert!(
        out.stderr
            .starts_with("spike-ai-pipeline-mail: error: the store's root can't be read: "),
        "{}",
        out.stderr
    );
    assert_eq!(out.stderr.lines().count(), 1);
}

/// A compound file cfb can't read is refused: here, a bare header, its sectors all zero.
#[test]
fn an_unreadable_compound_file_is_refused() {
    let mut bytes = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    bytes.resize(512, 0);
    let msg = built("compound.msg", &bytes);
    let out = run(&[&msg]);
    assert_eq!(out.code, Some(3));
    assert_eq!(out.stdout, "");
    assert!(
        out.stderr.starts_with(&format!(
            "spike-ai-pipeline-mail: error: {}: the compound file can't be read: ",
            msg.display()
        )),
        "{}",
        out.stderr
    );
    assert_eq!(out.stderr.lines().count(), 1);
}

/// A compound file that holds no MS-OXMSG message, here one with a Word document's stream, is
/// refused, though it is named .msg.
#[test]
fn a_compound_file_that_is_not_a_message_is_refused() {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("not-a-message.msg");
    let mut file = cfb::create(&path).unwrap();
    std::io::Write::write_all(&mut file.create_stream("/WordDocument").unwrap(), b"text").unwrap();
    file.flush().unwrap();
    assert_refused(
        &[&path],
        &format!(
            "{}: a compound file, but not an Outlook message: \
             it has no __properties_version1.0 stream",
            path.display()
        ),
    );
}

#[test]
fn neither_a_pst_nor_a_msg_is_refused() {
    for (name, bytes) in [
        ("empty.pst", &b""[..]),
        ("text.pst", b"From: someone\r\n\r\nhello"),
        ("short.pst", b"!BDN\0\0\0\0SM"),
        ("client.pst", &pst_header(b"XX", 23)),
    ] {
        let input = built(name, bytes);
        assert_refused(
            &[&input],
            &format!("{}: neither a .pst nor a .msg", input.display()),
        );
    }
}

#[test]
fn a_missing_input_is_refused() {
    let missing = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("missing.pst");
    assert_refused(
        &[&missing],
        &format!(
            "{}: No such file or directory (os error 2)",
            missing.display()
        ),
    );
}

/// A path needn't be UTF-8: one that isn't is read like any other, here refused as missing.
#[test]
fn a_path_that_is_not_utf8_is_read_as_a_path() {
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::OsStr::from_bytes(b"missing-\xFF.pst");
    let missing = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    assert_refused(
        &[&missing],
        &format!(
            "{}: No such file or directory (os error 2)",
            missing.display()
        ),
    );
}
