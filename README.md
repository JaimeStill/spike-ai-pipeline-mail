# spike-ai-pipeline-mail

The mail tool of [spike-ai-pipeline](https://github.com/JaimeStill/spike-ai-pipeline): a
command-line utility that reads Outlook `.pst` stores, through Microsoft's
[outlook-pst](https://github.com/microsoft/outlook-pst-rs), `.msg` messages, through its own
MS-OXMSG reader over [cfb](https://crates.io/crates/cfb), and `.eml` messages and mbox files,
through Stalwart's [mail-parser](https://crates.io/crates/mail-parser), into one JSON model: a
store's folder tree, each message in the folder it is stored in, with its headers, bodies, and
threading fields as stored, and its attachments, written out as files on request. It builds no
threads; processing does.

It is published as one static musl binary each for linux-x64 and linux-arm64, which
spike-ai-pipeline-processing's `email` package runs like any other system tool. It calls no model.

It is released by tag, `vX.Y.Z`, with a [CHANGELOG](CHANGELOG.md). Its finding for the spike's
evidence 16, the fields it recovers per kind and its time and peak memory per store, is
[`docs/findings/mail-cli.md`](docs/findings/mail-cli.md). The spike's question, evidence, and
answer are in the root repository's README.

## Usage

```sh
spike-ai-pipeline-mail [--attachments <dir>] [--after <id>] <input>
spike-ai-pipeline-mail --version
```

It reads one input, a `.pst`, a `.msg`, an `.eml`, or an mbox, and writes JSON Lines of schema
`"1"` to stdout, and nothing else there. The input's format is sniffed from its content, never
from its name, in this order: a PST header; an OLE2 compound file, which must hold an MS-OXMSG
message; a first line starting `From `, an mbox's separator line; else an RFC 5322 header block,
an `.eml`: a first line that is a header field, and a `From` or `Date` field before the block's
blank line or its first line that is neither a field nor a folded continuation. The arguments may
come in any order; `--version`, alone, prints the bare version, `0.3.1`.

With `--attachments <dir>`, each distinct attachment content is written once into `<dir>`,
created if missing, as stored, before the line of the message it belongs to. Without it nothing is
written to disk.

With `--after <id>`, a read of a `.pst` or an mbox resumes after the message whose `id` is given:
in a `.pst` its node id, in an mbox its 1-based ordinal. The resumed read is the tail of the whole
read, byte for byte: the store line, every folder line, and every `skipped` line of a folder or of
a folder's table, all as the whole read writes them, since a consumer places messages by their
folders; and of the messages, only the lines after the given message's, a message line and the
`skipped` lines of a message and of its attachments, in the whole read's order. The messages up to
and including the given one are passed over unread: a `.pst`'s by their node ids in their
folders' contents tables, never opened; an mbox's by scanning its separator lines from the start
of the file, never parsed. Neither writes their warnings or their attachments' files. The id is
decimal digits naming a whole number from 1; `0`, a sign, or anything else is a usage error, as
is `--after` with a `.msg` or an `.eml`, a single message. An id that names no message is refused:
in a `.pst`, one no folder's contents table holds, a folder's node id among them; in an mbox, an
ordinal past its last, a separator line with nothing after it counted. A resume costs about what
its remaining messages cost, plus the pass to the cursor: on the 2.1 GB scale store of 50,000
messages, resumed after its last, 0.1 s for the `.pst` and 1.9 s for the same messages as a 2.7 GB
mboxrd, whose every byte before the cursor is scanned (the finding's "Resuming a read").

| Exit code | Meaning |
| --- | --- |
| 0 | Read, even when records were skipped. |
| 1 | stdout, or an attachment's content, couldn't be written. |
| 2 | A usage error; usage goes to stderr. `--after` with a `.msg` or an `.eml`, or with an id that isn't a whole number from 1 to 4,294,967,295, is one. |
| 3 | Refused, before any output: the input is missing or unreadable; none of the four formats (`neither a .pst, a .msg, an .eml nor an mbox`); an `.ost`; a PST version outlook-pst doesn't read (it reads 14 and 15, ANSI, and 23, Unicode); a store whose header, root, or IPM subtree can't be read; a compound file that isn't an MS-OXMSG message, or whose top-level properties can't be read; or an `.eml` of 4 GiB or more, or one mail-parser can't parse; or an `--after` id that names no message in the store. |
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

`<where>` names the record. In a `.pst` it is `store`, or a folder's path
(`/Top of Personal Folders/Inbox`) followed by `, contents table` or `, hierarchy table` for one
of the folder's tables, or by `, message <id>` and then `, attachment <id>` down to the record. In
a `.msg` it is `named properties`, or `message` followed by `, attachment <n>` and `, message` for
each level of nesting down to the record. An `.eml`'s is the same, from `message`; an mbox's is
`message <n>`, the message's ordinal, followed in the same way. `<what>` is the reason,
`skipped: <reason>` for a skipped record.

An mbox message that can't be read, one whose header block has no `From` or `Date` field or one
mail-parser can't parse, is skipped like a record in a store, and the run goes on; a read of the
file that fails partway gives a skipped line for the message it was reading, and ends there.

## Schema "1"

Every line is one JSON object, its `line` field naming its kind. A property with no value is
left out, never written as `null`, and so is an empty list. Strings are decoded by their code
page: Unicode properties as UTF-16, 8-bit ones by the message's code page, else the store's. A
`.msg`, which has no store, falls back from the message's code page to its locale's ANSI code
page, then its internet code page unless that is a Unicode encoding, then its parent message's,
then Windows-1252. An `.eml` or mbox message's parts are decoded from their transfer encoding and
charset, its headers' encoded words likewise, and its raw header values as UTF-8. A string in a
code page or charset that can't be decoded is decoded lossily, with a warning. Times are ISO 8601
UTC, to the second. The same input always gives byte-identical output.

A `.pst` gives a `store` line, then each folder depth-first from the IPM subtree's root: each
`folder` line before its messages' lines, and its messages before its subfolders, in its contents
table's order; a `skipped` line stands wherever a record can't be read. A `.msg` gives a `store`
line and one `message` line, then a `skipped` line for each top-level attachment that can't be
read, and so does an `.eml`. An mbox gives a `store` line, then each message in file order, a
`message` line and its skipped attachments' lines, or a `skipped` line in its place; it has no
folders and no `folder` lines.

### store

| Field | Value |
| --- | --- |
| `line` | `"store"` |
| `schema` | `"1"` |
| `format` | `"pst"`, `"msg"`, `"eml"`, or `"mbox"` |
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
| `id` | In a `.pst`, the message's node id; in an mbox, its 1-based ordinal in the file, which a separator line with nothing after it keeps, though it writes no line. An `.eml` has none. |
| `folder` | In a `.pst`, the node id of its folder; nothing else has one. |
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
| `inline` | Whether the HTML body refers to it (`ATT_MHTML_REF`), which is how an inline disposition is stored; in an `.eml` or mbox, whether its Content-Disposition is `inline`. Always written. |
| `method` | `"none"`, `"by_value"`, `"by_reference"`, `"by_reference_only"`, `"embedded_message"`, `"storage"`, or `"by_web_reference"`. |
| `size` | The content's size in bytes, when its content is read. |
| `message` | In a `.msg`, an `.eml`, or an mbox, an embedded message: a message object with every field above, its bodies and attachments included, nested up to 16 deep. |

A file's name is a function of its content, so attachments with the same bytes share one file,
written once. By-value and `afStorage` attachments are written as stored, an `afStorage` one in a
`.msg` as its storage copied into a compound file of its own. An attachment by reference has no
file. An embedded message is no file: in a `.msg`, an `.eml`, or an mbox it is nested under
`message`; in a `.pst` it is listed with its metadata alone, with a warning naming it.

### .eml and mbox messages

An `.eml` or mbox message fills the message fields from its headers and MIME parts, through
mail-parser 1.0.1. Where a header appears more than once, the last is read, except Received,
whose first gives `delivery_time`.

| Field | From |
| --- | --- |
| `class` | None: a MIME message has no message class. |
| `subject` | Subject, its encoded words decoded. |
| `sender` | The first mailbox of From. |
| `to`, `cc`, `bcc` | To's, Cc's, and Bcc's mailboxes in order, a group's members in its place. |
| `submit_time` | Date, in UTC. |
| `delivery_time` | The date of the first Received header, the newest hop's, in UTC. |
| `creation_time`, `modification_time` | None. |
| `transport_headers` | The raw header block, without the blank line that ends it. |
| `message_id`, `in_reply_to`, `references` | Message-ID, In-Reply-To, and References, as stored, unfolded. |
| `conversation_topic` | Thread-Topic, its encoded words decoded. |
| `conversation_index` | Thread-Index, base64-decoded, in lowercase hex. |
| `importance` | Importance's `low`, `normal`, or `high`; else X-Priority's leading number, 1 or 2 `"high"`, 3 `"normal"`, 4 or 5 `"low"`. |
| `sensitivity` | Sensitivity's `Personal`, `Private`, and `Company-Confidential`, as `"personal"`, `"private"`, and `"confidential"`. |

Every address has address type `SMTP`. A date, Thread-Index, Importance, X-Priority, or
Sensitivity value that doesn't read or map is left out, with a warning; a Sensitivity of `Normal`
is one of these, and an unknown Importance falls back to X-Priority. A raw header value that isn't valid UTF-8 is decoded lossily, with a
warning.

The bodies are mail-parser's body lists (RFC 8621's textBody and htmlBody): `text` is the text/plain
parts of the one and `html` the text/html parts of the other, each decoded and joined in order by a
line break, and `derived.markdown` is made from them as for a `.pst` or a `.msg`. There is no
`rtf` and no `derived.html`. The attachments are the message's parts that aren't body text, in
order: its attachments, and the images, audio, and video its bodies show. Each is `by_value`, its
content decoded from its transfer encoding, with a warning when that is decoded lossily; its
filename, MIME type, and content-id are its headers', and `inline` is true when its
Content-Disposition is `inline`. A message/rfc822 or message/global part is `embedded_message`,
nested under `message` up to 16 deep like a `.msg`'s; one that can't be parsed is listed without
it, with a warning.

### skipped

| Field | Value |
| --- | --- |
| `line` | `"skipped"` |
| `kind` | `"folder"`, `"message"`, or `"attachment"`; or `"contents"` or `"hierarchy"` for a folder's table. |
| `folder` | The node id of the folder the record is in; a skipped folder names its parent, and a skipped table the folder whose table it is. |
| `message` | An attachment's message's node id; in an mbox, its ordinal. |
| `id` | Absent for a table. In a `.pst`, the record's own node id, an attachment's within its message; in a `.msg`, an attachment's number, the one its storage's name ends in; in an `.eml` or mbox message, an attachment's 1-based number among its attachment parts, and in an mbox, a message's ordinal. |
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
- mail-parser decodes US-ASCII and ISO-8859-1 as windows-1252, as WHATWG labels them, and the
  five bytes windows-1252 leaves undefined, 0x81, 0x8D, 0x8F, 0x90, and 0x9D, as U+FFFD. In a
  body or attachment part they give a warning that the text is decoded lossily; in a header's
  encoded words, such as a Subject's, they give none, as mail-parser reports no problems there.
- An mbox is read as an mboxrd: a line quoted `>From `, `>>From `, and so on loses one `>`. An
  mboxo is read the same way, so a body line that was `>From ` to begin with loses its `>`.
- An mbox message runs to the next line starting `From `: mboxcl's and mboxcl2's Content-Length is
  ignored, so in one of those an unquoted body line starting `From ` starts a new message.
- An `.eml` of 4 GiB or more is refused: mail-parser's offsets are 32-bit.
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
`mise run mboxrd` writes the MIME group's mboxrd, `tests/fixtures/mime/nine.mboxrd`, from its nine
`.eml`; the check rebuilds it and compares it byte for byte.
`MISE_ENV=fixtures mise run fixtures` regenerates the generated fixtures from their `.eml` and
item sources, and `MISE_ENV=fixtures mise run scale` builds the scale store and the same messages as an mboxrd,
neither committed.
`mise run findings` runs the finding ([`docs/findings/mail-cli.md`](docs/findings/mail-cli.md),
"Rerunning").

Apache-2.0.
