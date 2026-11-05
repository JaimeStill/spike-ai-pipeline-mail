//! spike-ai-pipeline-mail: Outlook .pst stores, .msg messages, .eml messages and mbox files to a
//! JSON model.
//!
//! It reads one input, sniffed by content, and writes JSON Lines of schema "1" to stdout:
//! nothing else goes there. Warnings and errors are single lines on stderr. Exit codes:
//! 0 read, even with records skipped; 1 output that can't be written, to stdout or to the
//! attachments directory; 2 a usage error;
//! 3 an input refused; 4 a fault of the CLI's own.

mod codepage;
mod eml;
mod files;
mod guard;
mod jsonl;
mod mapping;
mod markdown;
mod mbox;
mod mime;
mod model;
mod msg;
mod props;
mod pst;
mod rtf;
mod sniff;

use std::any::Any;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read};
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::process::ExitCode;

use files::Files;
use jsonl::Lines;
use sniff::Format;

const NAME: &str = "spike-ai-pipeline-mail";

const USAGE: &str = "usage: spike-ai-pipeline-mail [--attachments <dir>] [--after <id>] <input>
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
    /// `--after <id>`: the message a .pst's or an mbox's read resumes after.
    after: Option<u32>,
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
            // The bare version, which the release compares with its tag's.
            println!("{}", env!("CARGO_PKG_VERSION"));
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
        Format::Pst(encoding) => pst::read(path, encoding, args.after, &mut out, &mut files)?,
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
            single(args.after, &input, "a .msg")?;
            msg::read(path, &mut out, &mut files)?;
        }
        Format::Mbox => mbox::read(path, args.after, &mut out, &mut files)?,
        Format::Other => {
            let shaped = File::open(path)
                .and_then(|file| sniff::header_shape(BufReader::new(file)))
                .map_err(|err| Fault::Refused(format!("{input}: {err}")))?;
            if !shaped {
                return Err(Fault::Refused(format!(
                    "{input}: neither a .pst, a .msg, an .eml nor an mbox"
                )));
            }
            single(args.after, &input, "an .eml")?;
            eml::read(path, &mut out, &mut files)?;
        }
    }
    out.flush().map_err(Fault::Output)
}

/// Refuses `--after` for an input that is a single message, `kind`, as a usage error: there is no
/// message to resume after. The input has been sniffed first, so a missing or foreign input is
/// still refused as such.
fn single(after: Option<u32>, input: &impl std::fmt::Display, kind: &str) -> Result<(), Fault> {
    match after {
        Some(_) => Err(Fault::Usage(format!(
            "--after resumes a .pst or an mbox; {input} is {kind}, a single message"
        ))),
        None => Ok(()),
    }
}

/// Parses the arguments by hand: `--version` alone, or one input and an optional
/// `--attachments <dir>` and `--after <id>`, in any order.
fn parse(args: &[OsString]) -> Result<Command, Fault> {
    if let [flag] = args
        && flag == "--version"
    {
        return Ok(Command::Version);
    }
    let usage = |reason: &str| Err(Fault::Usage(reason.to_owned()));
    let mut input = None;
    let mut attachments = None;
    let mut after = None;
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
            Some("--after") => {
                let Some(id) = rest.next().filter(|id| !id.is_empty()) else {
                    return usage("--after needs a message id");
                };
                let Some(id) = message_id(id) else {
                    return usage(&format!(
                        "--after {} is not a message id, a whole number from 1",
                        id.to_string_lossy()
                    ));
                };
                if after.replace(id).is_some() {
                    return usage("--after is given more than once");
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
    Ok(Command::Read(Args {
        input,
        attachments,
        after,
    }))
}

/// `--after`'s id: decimal digits alone, no sign, naming a u32 from 1. A .pst's node ids and an
/// mbox's ordinals both start at 1, so 0 names no message and is a usage error, like an id that
/// isn't a number, rather than "resume from the start".
fn message_id(arg: &OsString) -> Option<u32> {
    let digits = arg.to_str()?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().filter(|id| *id != 0)
}

/// Writes one warning line: "spike-ai-pipeline-mail: warning: <where>: <what>".
pub fn warn(place: &str, what: &str) {
    eprintln!("{NAME}: warning: {place}: {what}");
}

/// Writes one warning line for each of a record's notes.
pub fn warn_all(place: &str, notes: Vec<String>) {
    for note in notes {
        warn(place, &note);
    }
}

/// Writes one warning line for each note of a record and of the records below it, each note's
/// place following `place`.
pub fn warn_below(place: &str, notes: Vec<(String, String)>) {
    for (below, note) in notes {
        warn(&format!("{place}{below}"), &note);
    }
}

/// The message a panic was raised with.
pub fn panic_text(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic without a message".to_owned())
}
