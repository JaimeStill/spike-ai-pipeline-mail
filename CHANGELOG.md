# Changelog

All notable changes to `spike-ai-pipeline-mail` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the CLI adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). The JSON Lines of schema `"1"`, the
`schema` version included, the arguments, the exit codes, the warning format, and the attachment
file names are the CLI's output contract: a change to any of them is a change here.

## [Unreleased]

## [v0.2.0] - 2026-11-04

The output contract only grows: a `.pst` or a `.msg` gives the same lines, byte for byte, as in
v0.1.0.

### Added

- `.eml` messages and mbox files as inputs, read through mail-parser 1.0.1 (Stalwart's RFC 5322
  and MIME parser, Apache-2.0 OR MIT, with its `full_encoding` feature), sniffed by content after
  the binary formats: an mbox by a first line starting `From `, an `.eml` by a header block whose
  first line is a header field and which has a `From` or `Date` field.
- Schema `"1"`, additively: the `store` line's `format` may be `"eml"` or `"mbox"`, with no
  encoding, password flag, or display name. An `.eml` gives a `store` line and one `message`
  line, with no id and no folder; an mbox gives a `store` line and each message in file order,
  its `id` its 1-based ordinal in the file, which a separator line with nothing after it keeps
  though it writes no line, with no folder and no `folder` lines.
- The MIME field mapping: the sender from From's first mailbox, the To, Cc, and Bcc mailboxes,
  groups flattened, each with address type `SMTP`; the Date header as the submit time and the
  first Received header's date as the delivery time, in UTC; the raw header block as the
  transport headers; Message-ID, In-Reply-To, and References as stored, unfolded; Thread-Topic as
  the conversation topic and Thread-Index, base64-decoded, as the conversation index in hex;
  Importance, else X-Priority's bands (1 to 2 high, 3 normal, 4 to 5 low), as the importance; and
  Sensitivity's Personal, Private, and Company-Confidential as personal, private, and
  confidential. A MIME message has no class and no creation or modification time. A value that
  doesn't read or map is left out, with a warning.
- MIME bodies and attachments: the text and HTML bodies are the text and HTML parts of RFC 8621's
  body lists, decoded and joined in order, with the Markdown body derived from them; the
  attachments are the parts that aren't body text, `by_value`, `inline` by their
  Content-Disposition, and message/rfc822 and message/global parts are `embedded_message`,
  nested under their record up to 16 deep. Text or content mail-parser decodes with a problem is decoded lossily, with a
  warning.
- Refusals, exit 3: an input of none of the four formats, `neither a .pst, a .msg, an .eml nor an
  mbox` (it was `neither a .pst nor a .msg`); an `.eml` of 4 GiB or more; and an `.eml`
  mail-parser can't parse.
- Warnings name an `.eml`'s records from `message`, and an mbox's from `message <n>`, each
  followed by `, attachment <n>` and `, message` down to the record. An mbox message that can't be
  read is a `skipped` line of kind `"message"`, its `id` its ordinal, and the run goes on; an
  attachment's `skipped` line in an mbox names its message's ordinal.
- Fixtures: the MIME group, `tests/fixtures/mime/`, with `09-threading.eml` and its hand truth;
  `nine.mboxrd`, an mboxrd of the eight generated `.eml` and that one; and Apache James mime4j's
  `mbox.rlug` and its five splits, with their license, notice, sources, and SHA-256. The generator's
  scale mode also writes the scale store's messages as `scale.mboxrd`.
- `scripts/mboxrd.sh`, `mise run mboxrd`, which writes `nine.mboxrd` from its sources; the check
  rebuilds it and compares it byte for byte.
- The finding's ".eml and mbox" section (`docs/findings/mail-cli.md`), from recorded run v0.2.0:
  the fields recovered against the MIME fixtures' truth and the time and peak memory on the scale
  messages as a 2.7 GB mboxrd.

### Known limits

- mboxcl's and mboxcl2's Content-Length is ignored: an mbox message runs to the next line starting
  `From `.
- In an mboxo, a body line that was `>From ` to begin with loses its `>`: every mbox is read as
  an mboxrd.
- The five bytes windows-1252 leaves undefined, 0x81, 0x8D, 0x8F, 0x90, and 0x9D, decode as
  U+FFFD, with a lossy-decoding warning; mail-parser decodes US-ASCII and ISO-8859-1 as
  windows-1252.

## [v0.1.0] - 2026-11-03

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
  read, its `kind` `"folder"`, `"message"`, `"attachment"`, `"contents"`, or `"hierarchy"`. A
  property with no value is left out, never written as `null`, and the same input always gives
  byte-identical output.
- Messages: class, subject, sender and To, Cc, and Bcc recipients (name, address, address type),
  the submit, delivery, creation, and modification times in ISO 8601 UTC, importance,
  sensitivity, the raw transport headers, and the threading fields as stored (Message-ID,
  In-Reply-To, References, conversation topic, conversation index in hex). Strings are decoded by
  their code page, an undecodable one lossily with a warning: a `.pst` message's 8-bit strings by
  its code page, else the store's; a `.msg`'s by its code page, else its locale's ANSI code page,
  else its internet code page unless it is a Unicode encoding, else its parent message's, else
  Windows-1252.
- Bodies: the stored plain text, HTML (by its internet code page, else its `<meta>` charset, else
  as UTF-8 when valid, else by the message's code page), and RTF (decompressed); under `derived`,
  the HTML recovered from RTF that encapsulates it (`\fromhtml1`) and a Markdown body for every
  message, made from the HTML, else the HTML from RTF, else the plain text, escaped to read
  literally.
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

[Unreleased]: https://github.com/JaimeStill/spike-ai-pipeline-mail/compare/v0.2.0...HEAD
[v0.2.0]: https://github.com/JaimeStill/spike-ai-pipeline-mail/releases/tag/v0.2.0
[v0.1.0]: https://github.com/JaimeStill/spike-ai-pipeline-mail/releases/tag/v0.1.0
