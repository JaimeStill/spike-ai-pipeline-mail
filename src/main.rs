//! spike-ai-pipeline-mail: Outlook .pst stores and .msg messages to a JSON model.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [flag] if flag == "--version" => {
            println!("spike-ai-pipeline-mail {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: spike-ai-pipeline-mail --version");
            ExitCode::from(2)
        }
    }
}
