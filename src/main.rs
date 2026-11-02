//! spike-ai-pipeline-mail: Outlook .pst stores and .msg messages to a JSON model.
//!
//! It reads one input, sniffed by content, and writes JSON Lines of schema "1" to stdout:
//! nothing else goes there. Warnings and errors are single lines on stderr. Exit codes:
//! 0 read, even with records skipped; 1 output that can't be written, to stdout or to the
//! attachments directory; 2 a usage error;
//! 3 an input refused; 4 a fault of the CLI's own.

mod codepage;
mod files;
mod jsonl;
mod mapping;
mod markdown;
mod model;
mod props;
mod pst;
mod rtf;
mod sniff;

use std::any::Any;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufWriter, Read};
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::process::ExitCode;

use files::Files;
use jsonl::Lines;
use sniff::Format;

const NAME: &str = "spike-ai-pipeline-mail";

const USAGE: &str = "usage: spike-ai-pipeline-mail [--attachments <dir>] <input>
       spike-ai-pipeline-mail --version";

/// Why a run stopped short of exit 0.
pub enum Fault {
    /// The arguments: exit 2, with usage.
    Usage(String),
    /// The input: exit 3, with one reason line and nothing on stdout.
    Refused(String),
    /// stdout couldn't be written: exit 1.
    Output(io::Error),
    /// An attachment's content couldn't be written to the attachments directory: exit 1.
    Attachments(String),
    /// A fault of the CLI's own: exit 4.
    Internal(String),
}

/// The parsed command line.
enum Command {
    Version,
    Read(Args),
}

struct Args {
    input: PathBuf,
    attachments: Option<PathBuf>,
}

fn main() -> ExitCode {
    // Panics are reported once, as a warning or an error line, never by the default hook.
    panic::set_hook(Box::new(|_| {}));
    // args_os: a path needn't be UTF-8, and std::env::args panics on one that isn't.
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| run(&args)))
        .unwrap_or_else(|payload| Err(Fault::Internal(panic_text(&*payload))));
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(Fault::Output(err)) => fail(1, &format!("stdout can't be written: {err}")),
        Err(Fault::Attachments(reason)) => fail(1, &reason),
        Err(Fault::Usage(reason)) => {
            eprintln!("{NAME}: {reason}\n{USAGE}");
            ExitCode::from(2)
        }
        Err(Fault::Refused(reason)) => fail(3, &reason),
        Err(Fault::Internal(reason)) => fail(4, &format!("internal fault: {reason}")),
    }
}

fn fail(code: u8, reason: &str) -> ExitCode {
    eprintln!("{NAME}: error: {reason}");
    ExitCode::from(code)
}

fn run(args: &[OsString]) -> Result<(), Fault> {
    let args = match parse(args)? {
        Command::Version => {
            println!("{NAME} {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Command::Read(args) => args,
    };
    let path = &args.input;
    let input = path.display();
    let mut prefix = Vec::with_capacity(sniff::PREFIX);
    File::open(path)
        .and_then(|file| file.take(sniff::PREFIX as u64).read_to_end(&mut prefix))
        .map_err(|err| Fault::Refused(format!("{input}: {err}")))?;
    let mut out = Lines::new(BufWriter::new(io::stdout().lock()));
    let mut files = Files::new(args.attachments);
    match sniff::sniff(&prefix) {
        Format::Pst(encoding) => pst::read(path, encoding, &mut out, &mut files)?,
        Format::Ost => {
            return Err(Fault::Refused(format!(
                "{input}: an .ost, the offline cache of a server mailbox, is not read; \
                 export the mailbox from Outlook as a .pst"
            )));
        }
        Format::PstVersion(version) => {
            return Err(Fault::Refused(format!(
                "{input}: PST version {version} is not read; \
                 only versions 14 and 15 (ANSI) and 23 (Unicode) are"
            )));
        }
        Format::CompoundFile => {
            return Err(Fault::Refused(format!(
                "{input}: a compound file, as a .msg is; .msg is not yet supported"
            )));
        }
        Format::Unknown => {
            return Err(Fault::Refused(format!(
                "{input}: neither a .pst nor a .msg"
            )));
        }
    }
    out.flush().map_err(Fault::Output)
}

/// Parses the arguments by hand: `--version` alone, or one input and an optional
/// `--attachments <dir>`, in any order.
fn parse(args: &[OsString]) -> Result<Command, Fault> {
    if let [flag] = args
        && flag == "--version"
    {
        return Ok(Command::Version);
    }
    let usage = |reason: &str| Err(Fault::Usage(reason.to_owned()));
    let mut input = None;
    let mut attachments = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.to_str() {
            Some("--attachments") => {
                let Some(dir) = rest.next().filter(|dir| !dir.is_empty()) else {
                    return usage("--attachments needs a directory");
                };
                if attachments.replace(PathBuf::from(dir)).is_some() {
                    return usage("--attachments is given more than once");
                }
            }
            Some("--version") => return usage("--version takes no other arguments"),
            Some(option) if option.starts_with('-') && option != "-" => {
                return usage(&format!("unknown option {option}"));
            }
            _ => {
                if input.replace(PathBuf::from(arg)).is_some() {
                    return usage("more than one input");
                }
            }
        }
    }
    let Some(input) = input else {
        return usage("no input");
    };
    if let Some(dir) = &attachments
        && dir.exists()
        && !dir.is_dir()
    {
        return usage(&format!(
            "--attachments {} is not a directory",
            dir.display()
        ));
    }
    Ok(Command::Read(Args { input, attachments }))
}

/// Writes one warning line: "spike-ai-pipeline-mail: warning: <where>: <what>".
pub fn warn(place: &str, what: &str) {
    eprintln!("{NAME}: warning: {place}: {what}");
}

/// The message a panic was raised with.
pub fn panic_text(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic without a message".to_owned())
}
