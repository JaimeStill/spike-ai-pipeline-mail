# spike-ai-pipeline-mail

The mail tool of [spike-ai-pipeline](https://github.com/JaimeStill/spike-ai-pipeline): a
command-line utility that reads Outlook `.pst` stores, through Microsoft's
[outlook-pst](https://github.com/microsoft/outlook-pst-rs), and `.msg` messages, through its own
MS-OXMSG reader over [cfb](https://crates.io/crates/cfb), into one JSON model: the store's folder
tree, each message in the folder it is stored in, with its headers, bodies, and threading fields
as stored, and its attachments, written out as files on request. It builds no threads; processing
does.

It is published as one static musl binary each for linux-x64 and linux-arm64, and
spike-ai-pipeline-processing runs it like any other system tool. It calls no model.

It is released by tag, `vX.Y.Z`, with a [CHANGELOG](CHANGELOG.md). Its finding for the spike's
evidence 16, the fields it recovers per kind and its time and peak memory per store, is
[`docs/findings/mail-cli.md`](docs/findings/mail-cli.md). The spike's question, evidence, and
answer are in the root repository's README.

## Usage

```sh
spike-ai-pipeline-mail [--attachments <dir>] <input>
spike-ai-pipeline-mail --version
```

It reads one input, a `.pst` or a `.msg`, and writes JSON Lines of schema `"1"` to stdout, and
nothing else there. The input's format is sniffed from its content, never from its name: a PST
header, or an OLE2 compound file holding an MS-OXMSG message. The arguments may come in any order;
`--version`, alone, prints the bare version, `0.1.0`.

With `--attachments <dir>`, each distinct attachment content is written once into `<dir>`,
created if missing, as stored, before the line of the message it belongs to. Without it nothing is
written to disk.

| Exit code | Meaning |
| --- | --- |
| 0 | Read, even when records were skipped. |
| 1 | stdout, or an attachment's content, couldn't be written. |
| 2 | A usage error; usage goes to stderr. |
| 3 | Refused, before any output: the input is missing or unreadable; neither a `.pst` nor a `.msg`; an `.ost`; a PST version outlook-pst doesn't read (it reads 14 and 15, ANSI, and 23, Unicode); a store whose header, root, or IPM subtree can't be read; or a compound file that isn't an MS-OXMSG message, or whose top-level properties can't be read. |
| 4 | A fault of the CLI's own, never reported as any other code. |

An error, for exit codes 1 to 4, is one line on stderr,
`spike-ai-pipeline-mail: error: <reason>`; a usage error's is followed by the usage. An `.ost`,
the offline cache of a server mailbox, is refused with a reason saying so: export the mailbox from
Outlook as a `.pst`. ANSI and password-protected stores are read like any other, and the store
line says which they are.

A folder, message, or attachment that can't be read inside a readable store, whether its reader
returns an error or panics, is skipped: a `skipped` line takes its place, a warning goes to
stderr, and the run goes on and exits 0. A folder whose contents or hierarchy table can't be read,
while its PidTagContentCount says it has items or its PidTagSubfolders says it has subfolders,
keeps its folder line and gets a `skipped` line where that table's records would be. Every warning
is one line:

```text
spike-ai-pipeline-mail: warning: <where>: <what>
```

`<where>` is `store`, or a folder's path (`/Top of Personal Folders/Inbox`) followed by
`, contents table`, `, hierarchy table`, or `, message <id>` and `, attachment <id>` as far down
as the record is; in a `.msg`, `message`,
`message, attachment <n>`, or `named properties`. `<what>` is the reason, `skipped: <reason>` for
a skipped record.

## Schema "1"

Every line is one JSON object, its `line` field naming its kind. A property with no value is left
out, never written as `null`, and so is an empty list. Strings are decoded by their code page:
Unicode properties as UTF-16, 8-bit ones by the message's code page, else the store's; a `.msg`,
which has no store, falls back to its locale's ANSI code page, then its internet code page unless
it is a Unicode encoding, then its parent message's, then Windows-1252. A string in a code page that can't
be decoded is decoded lossily, with a warning. Times are ISO 8601 UTC, to the second. The same
input always gives byte-identical output.

A `.pst` gives a `store` line, then each folder depth-first from the IPM subtree's root: each
`folder` line before its messages' lines, and its messages before its subfolders, in its contents
table's order; a `skipped` line stands wherever a record can't be read. A `.msg` gives a `store`
line and one `message` line, then a `skipped` line for each top-level attachment that can't be
read.

### store

| Field | Value |
| --- | --- |
| `line` | `"store"` |
| `schema` | `"1"` |
| `format` | `"pst"` or `"msg"` |
| `encoding` | A `.pst`'s: `"unicode"` or `"ansi"`. |
| `password` | A `.pst`'s: whether it is password-protected. |
| `display_name` | A `.pst`'s display name. |

### folder

| Field | Value |
| --- | --- |
| `line` | `"folder"` |
| `id` | The folder's node id. |
| `parent` | Its parent folder's node id; the IPM subtree's root has none. |
| `path` | The display names from the IPM subtree's root down to this folder, a list. |
| `display_name` | Its display name. |
| `container_class` | Its container class, such as `IPF.Note`. |

### message

Every item is a message line, whatever its message class, and no class-specific field is written.
Nothing is grouped into threads.

| Field | Value |
| --- | --- |
| `line` | `"message"`; a message nested under an attachment has none. |
| `id` | In a `.pst`, the message's node id. |
| `folder` | In a `.pst`, the node id of its folder. |
| `class` | The message class, such as `IPM.Note`. |
| `subject` | The subject, its prefix marker dropped. |
| `sender` | An address. |
| `to`, `cc`, `bcc` | Lists of addresses, in the recipient table's order. |
| `submit_time`, `delivery_time`, `creation_time`, `modification_time` | Times. |
| `importance` | `"low"`, `"normal"`, or `"high"`. |
| `sensitivity` | `"normal"`, `"personal"`, `"private"`, or `"confidential"`. |
| `transport_headers` | The raw transport headers, as stored. |
| `message_id`, `in_reply_to`, `references` | The Internet threading fields, as stored. |
| `conversation_topic` | As stored. |
| `conversation_index` | Lowercase hex of the stored bytes. |
| `body` | The bodies, below. |
| `attachments` | A list of attachment records, below, in the attachment table's order. |

An address has a `name`, an `address`, and an `address_type` (`SMTP`, `EX`), each as stored.

`body` holds the stored bodies at its top level, each as stored, and the CLI's own under
`derived`, so a stored and a derived body never share a place:

| Field | Value |
| --- | --- |
| `text` | The plain-text body. |
| `html` | The HTML body, decoded by its internet code page, else the charset its own `<meta>` declares, else as UTF-8 when it is valid UTF-8, else by the message's code page, then the store's. |
| `rtf` | The RTF body, decompressed. |
| `derived.html` | The HTML recovered from an RTF body that encapsulates HTML (`\fromhtml1`, MS-OXRTFEX). |
| `derived.markdown` | Markdown made from the stored HTML, else from the HTML recovered from RTF, else from the plain text, escaped to read literally. Images keep their `cid:` sources, which name attachments' `content_id`. |

### Attachment records

| Field | Value |
| --- | --- |
| `file` | The name of its content's file: the lowercase hex SHA-256 of the bytes, plus `.` and the stored extension when that is 1 to 8 ASCII letters and digits, in its stored case. An attachment whose content isn't a file has none. |
| `filename` | The long filename, else the short one. |
| `mime_type` | As stored. |
| `content_id` | As stored, without angle brackets. |
| `inline` | Whether the HTML body refers to it (`ATT_MHTML_REF`), which is how an inline disposition is stored; always written. |
| `method` | `"none"`, `"by_value"`, `"by_reference"`, `"by_reference_only"`, `"embedded_message"`, `"storage"`, or `"by_web_reference"`. |
| `size` | The content's size in bytes, when its content is read. |
| `message` | In a `.msg`, an embedded message: a message object with every field above, its bodies and attachments included, nested up to 16 deep. |

A file's name is a function of its content, so attachments with the same bytes share one file,
written once. By-value and `afStorage` attachments are written as stored, an `afStorage` one in a
`.msg` as its storage copied into a compound file of its own. An attachment by reference has no
file. An embedded message is no file: in a `.msg` it is nested under `message`; in a `.pst` it is
listed with its metadata alone, with a warning naming it.

### skipped

| Field | Value |
| --- | --- |
| `line` | `"skipped"` |
| `kind` | `"folder"`, `"message"`, or `"attachment"`; or `"contents"` or `"hierarchy"` for a folder's table. |
| `folder` | The node id of the folder the record is in; a skipped folder names its parent, and a skipped table the folder whose table it is. |
| `message` | An attachment's message's node id. |
| `id` | Absent for a table. In a `.pst`, the record's own node id, an attachment's within its message; in a `.msg`, an attachment's number, the one its storage's name ends in. |
| `reason` | Why it couldn't be read. |

## Known limits

- In a `.pst`, embedded messages are listed without their content: outlook-pst 1.2.0 can't read
  them (real stores fail on a missing `PidTagAttachMethod`, and the spec-shaped path locks a mutex
  it already holds), and the fix is unmerged upstream. The CLI reads the attach method from the
  attachment table and never opens one. In a `.msg` they are read in full.
- ANSI stores are read but untested: no ANSI store is among the fixtures.
- `.ost` files are refused.
- By-reference attachments have no file.
- Class-specific fields are absent: a contact, an appointment, or a task is written with the
  message fields alone.
- A string in a code page that can't be decoded is decoded lossily, with a warning.
- An Exchange (`EX`) address is written as stored, its X.500 distinguished name; its SMTP address
  (`PidTagSmtpAddress`) isn't written.
- An item outlook-pst 1.2.0 can't open is skipped, such as a contact stored without a sub-node
  tree (`Missing Sub-Node Tree on message`).
- A folder's table that can't be read is told from an empty one only by the folder's own count:
  outlook-pst 1.2.0 gives an unreadable table as no table, without its error, so the skipped
  line's reason can't name the fault. A folder whose count is wrong the other way, claiming no
  items or no subfolders while it has some, loses them silently.

## Development

`mise run check` lints, format-checks, tests, and builds the static musl release binary; CI runs
it on linux-x64 and linux-arm64. The tests run the built binary over the committed fixtures
(`tests/fixtures/`, each with its sources and SHA-256) and over inputs the tests build.
`MISE_ENV=fixtures mise run fixtures` regenerates the generated fixtures from their `.eml`
sources, and `MISE_ENV=fixtures mise run scale` builds the scale store, which is never committed.
`mise run findings` runs the finding ([`docs/findings/mail-cli.md`](docs/findings/mail-cli.md),
"Rerunning").

Apache-2.0.
