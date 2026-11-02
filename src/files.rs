//! The attachments directory of `--attachments <dir>`: each distinct content written once, by
//! its file name from the mapping, as stored. Without the flag nothing is written to disk.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use crate::Fault;

pub struct Files {
    dir: Option<PathBuf>,
    /// The names written in this run. A name is its content's SHA-256, so a name written once
    /// is never written again.
    written: HashSet<String>,
}

impl Files {
    /// The directory to write into, or None to write nothing.
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self {
            dir,
            written: HashSet::new(),
        }
    }

    /// Creates the directory if it is missing, once the input is known to be read, so a run
    /// that exits 0 always leaves it.
    pub fn create(&self) -> Result<(), Fault> {
        let Some(dir) = &self.dir else {
            return Ok(());
        };
        fs::create_dir_all(dir).map_err(|err| {
            Fault::Attachments(format!(
                "the attachments directory {} can't be created: {err}",
                dir.display()
            ))
        })
    }

    /// Writes one content under `name`, unless this run has already written it.
    pub fn write(&mut self, name: &str, content: &[u8]) -> Result<(), Fault> {
        let Some(dir) = &self.dir else {
            return Ok(());
        };
        if self.written.contains(name) {
            return Ok(());
        }
        let path = dir.join(name);
        fs::write(&path, content).map_err(|err| {
            Fault::Attachments(format!(
                "the attachment {} can't be written: {err}",
                path.display()
            ))
        })?;
        self.written.insert(name.to_owned());
        Ok(())
    }
}
