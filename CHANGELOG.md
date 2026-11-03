# Changelog

All notable changes to `spike-ai-pipeline-mail` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the CLI adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). The JSON Lines of schema `"1"`, the
`schema` version included, the arguments, the exit codes, the warning format, and the attachment
file names are the CLI's output contract: a change to any of them is a change here.

## [Unreleased]

## [v0.1.0] - 2026-11-02

### Added

- `spike-ai-pipeline-mail [--attachments <dir>] <input>`, which reads an Outlook `.pst` store,
  through outlook-pst 1.2.0, or a `.msg` message, through the CLI's own MS-OXMSG reader over cfb
  0.16.0, sniffed by content, never by name, and writes JSON Lines of schema `"1"` to stdout,
  which carries them and nothing else. Exit codes: 0 read, even with records skipped; 1 stdout or
  an attachment's content not written; 2 usage (usage on stderr); 3 refused (one reason line on
  stderr, nothing on stdout); and 4 a fault of the CLI's own. `--version` prints the bare
  version.
- The initial output contract, schema `"1"`: a `store` line first (format, and a `.pst`'s
  encoding, password flag, and display name); for a `.pst`, a `folder` line per folder,
  depth-first, before its messages (id, parent, path, display name, container class); a
  `message` line per item, whatever its class, in its folder's contents order; and a `skipped`
  line wherever a folder, message, attachment, or a folder's contents or hierarchy table can't be
  read. A property with no value is left
  out, never written as `null`, and the same input always gives byte-identical output.
- Messages: class, subject, sender and To, Cc, and Bcc recipients (name, address, address type),
  the submit, delivery, creation, and modification times in ISO 8601 UTC, importance,
  sensitivity, the raw transport headers, and the threading fields as stored (Message-ID,
  In-Reply-To, References, conversation topic, conversation index in hex). Strings are decoded by
  their code page, an undecodable one lossily with a warning: a `.pst` message's 8-bit strings by
  its code page, else the store's; a `.msg`'s by its code page, else its locale's ANSI code page,
  else its internet code page unless that is UTF-8.
- Bodies: the stored plain text, HTML (by its internet code page, else its `<meta>` charset), and
  RTF (decompressed); derived, the HTML recovered from RTF that encapsulates it (`\fromhtml1`) and
  a Markdown body for every message, from the HTML, else the HTML from RTF, else the plain text
  read literally.
- Attachments: each record's file name is the lowercase hex SHA-256 of its content plus its short
  alphanumeric stored extension, with its filename, MIME type, content-id, inline flag, attach
  method, and size. `--attachments <dir>` writes each distinct content once, as stored. In a
  `.msg`, embedded messages are nested under their attachment records, recursively, up to 16
  deep.
- Skips: a record whose reader errs or panics is a `skipped` line and one warning,
  `spike-ai-pipeline-mail: warning: <where>: <what>`, and the run still exits 0.
- Refusals: a missing or unreadable input, anything not a `.pst` or `.msg`, an `.ost` (export it
  from Outlook as a `.pst`), a PST version outlook-pst doesn't read, and a store or message whose
  header, root, IPM subtree, or top-level properties can't be read.
- Releases: `spike-ai-pipeline-mail-vX.Y.Z-linux-x64.tar.gz` and `-linux-arm64.tar.gz`, each a
  static musl binary and LICENSE, and `SHA256SUMS`.

### Known limits

- In a `.pst`, embedded messages are listed with their metadata and no content, with a warning:
  outlook-pst 1.2.0 can't read them, and its fix is unmerged upstream.
- ANSI stores are read but untested.
- `.ost` files are refused.
- By-reference attachments have no file.
- Class-specific fields are absent.
- A string in a code page that can't be decoded is decoded lossily.
- An Exchange address is written as stored; its SMTP address isn't written.
- An item outlook-pst 1.2.0 can't open, such as a contact without a sub-node tree, is skipped.
- A folder's table that can't be read is skipped only when the folder's count says it has items
  or subfolders, and its reason can't name the fault: outlook-pst 1.2.0 gives such a table as no
  table. A folder that claims no items or subfolders while it has some loses them silently.

[Unreleased]: https://github.com/JaimeStill/spike-ai-pipeline-mail/compare/v0.1.0...HEAD
[v0.1.0]: https://github.com/JaimeStill/spike-ai-pipeline-mail/releases/tag/v0.1.0
