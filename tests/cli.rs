use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_spike-ai-pipeline-mail");

#[test]
fn version_prints_the_crate_version() {
    let out = Command::new(BIN).arg("--version").output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("spike-ai-pipeline-mail {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn no_arguments_is_a_usage_error() {
    let out = Command::new(BIN).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}
