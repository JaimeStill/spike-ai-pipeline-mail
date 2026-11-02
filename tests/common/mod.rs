//! What the tests share: running the built binary, and the paths of fixtures, goldens and
//! test-built inputs.

#![allow(
    dead_code,
    reason = "each test crate compiles this module and uses only part of it"
)]

pub mod msg;

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
    finish(Command::new(BIN).args(args))
}

/// One run of the binary with `dir` as its working directory.
pub fn run_in<S: AsRef<std::ffi::OsStr>>(dir: &Path, args: &[S]) -> Run {
    finish(Command::new(BIN).current_dir(dir).args(args))
}

fn finish(command: &mut Command) -> Run {
    let out = command.output().unwrap();
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

/// An empty directory under Cargo's temporary directory for integration tests, emptied if a
/// previous run left it.
pub fn scratch(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    if path.exists() {
        std::fs::remove_dir_all(&path).unwrap();
    }
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// The names of the files in `dir`, sorted.
pub fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort_unstable();
    names
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
