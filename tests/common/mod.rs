//! What the tests share: running the built binary, and the paths of fixtures, goldens and
//! test-built inputs.

#![allow(
    dead_code,
    reason = "each test crate compiles this module and uses only part of it"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_spike-ai-pipeline-mail");

/// One run of the binary: its exit code, stdout and stderr.
pub struct Run {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    /// stdout's lines, each parsed as JSON.
    pub fn lines(&self) -> Vec<serde_json::Value> {
        self.stdout
            .lines()
            .map(|line| serde_json::from_str(line).expect("each stdout line is JSON"))
            .collect()
    }
}

pub fn run<S: AsRef<std::ffi::OsStr>>(args: &[S]) -> Run {
    let out = Command::new(BIN).args(args).output().unwrap();
    Run {
        code: out.status.code(),
        stdout: String::from_utf8(out.stdout).expect("stdout is UTF-8"),
        stderr: String::from_utf8(out.stderr).expect("stderr is UTF-8"),
    }
}

/// A committed fixture, by its path under tests/fixtures.
pub fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(path)
}

/// A test-built input, written under Cargo's temporary directory for integration tests.
pub fn built(name: &str, bytes: &[u8]) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// A file's lowercase hex SHA-256, by sha256sum, so the tests take no hashing crate.
pub fn sha256(path: &Path) -> String {
    let out = Command::new("sha256sum").arg(path).output().unwrap();
    assert!(out.status.success(), "sha256sum {}", path.display());
    String::from_utf8(out.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}
