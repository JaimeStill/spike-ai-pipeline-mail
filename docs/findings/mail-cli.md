# Finding: the mail CLI's fields against the truth, and its time per store

This page records the CLI's part of the spike's evidence 16 (the root's `context/README.md`,
item 16): the fields it recovers, per field kind, against the hand truth of the generated and
public fixtures, and its time and peak memory on every committed store, on plaso's 59 MB store,
and on a generated 2.1 GB store; and, for .eml and mbox, the fields it recovers against the MIME
fixtures' truth and its time and peak memory on the same generated messages as a 2.7 GB mboxrd. It measures the CLI alone, with no baseline against other
readers. Thread grouping and attachment routing, the rest of evidence 16, belong to processing's
planned email finding.

Every number here comes from run
[`mail-cli/runs/v0.1.0/results.json`](mail-cli/runs/v0.1.0/results.json), on the laptop on
2026-11-03 from 00:25:43 to 00:27:39 UTC, CLI 0.1.0 at commit 32e5aa9, the static musl release
binary the check builds for x86_64 (3,576,888 bytes, SHA-256 `fb56ab08...fb75cb8`); that file has
every miss and every run. The numbers of ".eml and mbox" come from run
[`mail-cli/runs/v0.2.0/results.json`](mail-cli/runs/v0.2.0/results.json), as that section says.
A run is kept only when it is recorded ("Rerunning").

## At a glance

| Question | Answer |
|---|---|
| Does the CLI recover what the files hold? | **Yes.** Over the generated fixtures, every field the truth expects is recovered or is one the generator doesn't store (180 expected: 147 recovered, 33 generator gaps, 0 mismatched, 0 missing). Over Tika's stores' threading truth, 47 of 47, and every absence as the truth says. Over Tika's .msg assertions, 21 of 22. Over Apache POI's .msg assertions, 31 of 31. |
| What does it miss? | One field, by design: an Exchange recipient's SMTP address. The CLI writes the address as stored, `PidTagEmailAddress` (an X.500 DN for an `EX` recipient); Tika reports `PidTagSmtpAddress`, which the CLI doesn't write. |
| How fast? | plaso's 59 MB store, 540 messages: **1.59 s**. The 2.1 GB scale store, 50,000 messages: **13.27 s**, **14.38 s** with `--attachments` writing 1.88 GB. Median of 3 runs each. |
| How much memory? | Peak resident memory **42 MiB** on plaso's store and **65 to 67 MiB** on the scale store: it barely grows with the store. |
| And .eml and mbox? | Every field recovered: the eight .eml and 09-threading.eml against their hand truth (101 of 101), and every message of nine.mboxrd and mime4j's mbox.rlug equal, field by field, to its source. The same 50,000 messages as a 2.7 GB mboxrd read in **6.86 s** at **41.0 MiB**, against **14.82 s** at **65.9 MiB** for scale.pst in the same run (run v0.2.0). |
| Anything else? | The static musl binary takes 2.0 to 2.3 times the glibc build's time on the large stores (13.27 s against 6.51 s; 1.59 s against 0.71 s), at about a tenth less memory. plaso's store has 3 contacts outlook-pst 1.2.0 can't open, skipped with a warning each. |

## The machine

The laptop: Intel Core Ultra 9 185H, 22 logical CPUs, 32 GB of memory (32,284,144 kB), Linux
7.2.15-1-omarchy (Omarchy, on Arch), the stores on a btrfs file system on a device-mapper
volume, on an NVMe disk.
The scale store and plaso's were read from the page cache: the scale store had just been written
by the generator, and each store's runs followed one another, so no run read the disk cold (that
needs root to drop the cache). `--attachments` writes went to the page cache too, unsynced.

## Fields recovered per kind

Each row counts the fields of one kind the truth expects, summed over the files of its table:

- **Recovered**: the CLI wrote the truth's value (bodies compared with CRLF made LF, as the truth
  writes line breaks).
- **Generator gap**: the generated file doesn't hold the truth's value, and the CLI wrote what the
  file holds instead: nothing, or the other value tests/generated.rs's header documents for that
  generator (and asserts the file holds). A gap is counted only on that exact other value, so it
  never hides a CLI fault.
- **Mismatched**: the CLI wrote another value. **Missing**: it wrote none.

### The generated fixtures, against truth.json

`tests/fixtures/generated/truth.json` is the hand truth of the eight `.eml` sources, written from
the sources as a mail reader reads them, not from the CLI's output. `pst/mailbox.pst` holds all
eight, built by PST Builder 1.3.0 (PstBuilder.Eml); each also became one `msg/*.msg`, built by
MsgKit 3.0.6. A message is found by its subject; a recipient is compared by name and address, by
address alone where the source gives no name; an attachment is found by its content's SHA-256,
then its parts compared.

| Field kind | .pst: expected | recovered | generator gap | mismatched or missing | .msg: expected | recovered | generator gap | mismatched or missing |
|---|---|---|---|---|---|---|---|---|
| Message (found by subject) | 8 | 8 | 0 | 0 | 8 | 8 | 0 | 0 |
| Folder | 8 | 8 | 0 | 0 | | | | |
| Subject | 8 | 8 | 0 | 0 | 8 | 8 | 0 | 0 |
| Sender | 8 | 8 | 0 | 0 | 8 | 8 | 0 | 0 |
| Recipient (To) | 9 | 9 | 0 | 0 | 9 | 9 | 0 | 0 |
| Recipient (Cc) | 2 | 2 | 0 | 0 | 2 | 2 | 0 | 0 |
| Recipient (Bcc) | 1 | 0 | 1 | 0 | 1 | 1 | 0 | 0 |
| Submit time | 8 | 8 | 0 | 0 | 8 | 8 | 0 | 0 |
| Delivery time | 6 | 6 | 0 | 0 | 6 | 0 | 6 | 0 |
| Importance | 1 | 0 | 1 | 0 | 1 | 1 | 0 | 0 |
| Message-ID | 8 | 0 | 8 | 0 | 8 | 0 | 8 | 0 |
| In-Reply-To | 1 | 0 | 1 | 0 | 1 | 0 | 1 | 0 |
| References | 2 | 0 | 2 | 0 | 2 | 0 | 2 | 0 |
| Body, plain text | 7 | 7 | 0 | 0 | 7 | 7 | 0 | 0 |
| Body, HTML | 3 | 3 | 0 | 0 | 3 | 3 | 0 | 0 |
| Attachment content (SHA-256 and size) | 3 | 3 | 0 | 0 | 3 | 3 | 0 | 0 |
| Attachment filename | 3 | 3 | 0 | 0 | 3 | 3 | 0 | 0 |
| Attachment MIME type | 3 | 3 | 0 | 0 | 3 | 2 | 1 | 0 |
| Attachment content-id | 1 | 1 | 0 | 0 | 1 | 1 | 0 | 0 |
| Attachment inline flag | 3 | 3 | 0 | 0 | 3 | 3 | 0 | 0 |
| Embedded message | 1 | 0 | 1 | 0 | 1 | 0 | 1 | 0 |
| **All** | **94** | **80** | **14** | **0** | **86** | **67** | **19** | **0** |

The gaps, each the generator's (tests/generated.rs, header comment):

- **PST Builder** writes no Bcc and no threading fields, always normal importance, and skips a
  message/rfc822 part, so the store has no embedded message.
- **MsgKit** takes the Date header as the delivery time, stores Message-ID and In-Reply-To without
  angle brackets and References as its last ID alone, types a `.csv` attachment
  `application/octet-stream` by its own table, and attaches a forwarded message as a by-value
  `.eml` file named by its subject, not as an embedded message.

No generated file therefore tests the threading fields or embedded messages. The public
fixtures below test them, as do the embedded messages `tests/msg.rs` builds.

### Tika's stores, against the threading truth

`tests/fixtures/pst/tika-threading.json` is hand truth for every message of Tika's
`testPST.pst` (7 messages) and `testPST_variousBodyTypes.pst` (4), checked against each message's
own transport headers and Thread-Index (its note says how). A null in it is a field the store
doesn't hold, counted under "absent".

| Field kind | Expected | Recovered | Mismatched | Missing | Absent, as the truth says |
|---|---|---|---|---|---|
| Message-ID | 11 | 11 | 0 | 0 | |
| In-Reply-To | 7 | 7 | 0 | 0 | 4 of 4 |
| References | 7 | 7 | 0 | 0 | 4 of 4 |
| Conversation topic | 11 | 11 | 0 | 0 | |
| Conversation index | 11 | 11 | 0 | 0 | |
| **All** | **47** | **47** | **0** | **0** | **8 of 8** |

### Tika's .msg files, against Tika's assertions

`tests/fixtures/msg/tika-truth.json` holds what Apache Tika's own `OutlookParserTest` (apache/tika
48010f83, its SHA-256 recorded there) asserts of the four committed Tika `.msg` files, each value
mapped to the CLI field that holds it, written as a JSON pointer into the message line.
Assertions on content, on Tika-only metadata, or by prefix are left out. `tests/msg.rs` checks the
same file.

| Field kind | Expected | Recovered | Mismatched | Missing |
|---|---|---|---|---|
| Subject | 1 | 1 | 0 | 0 |
| Sender name | 1 | 1 | 0 | 0 |
| Sender address | 1 | 1 | 0 | 0 |
| To name | 1 | 1 | 0 | 0 |
| To address | 1 | 0 | 1 | 0 |
| Message-ID | 1 | 1 | 0 | 0 |
| In-Reply-To | 1 | 1 | 0 | 0 |
| References | 1 | 1 | 0 | 0 |
| Conversation index | 1 | 1 | 0 | 0 |
| Attachment inline flag | 11 | 11 | 0 | 0 |
| Attachment method | 1 | 1 | 0 | 0 |
| Attachment filename | 1 | 1 | 0 | 0 |
| **All** | **22** | **21** | **1** | **0** |

The mismatch is `testMSG_chinese.msg`'s To address: Tika asserts `tests.chang@fengttt.com`, the
recipient's `PidTagSmtpAddress`; the CLI writes its `PidTagEmailAddress` as stored,
`/O=FT GROUP/OU=FT/CN=RECIPIENTS/CTestsiaChang` with address type `EX` (an independent reader,
olefile, shows the stream holds exactly that). The CLI writes addresses as stored and has no SMTP
address field: a known limit, in the README.

### Apache POI's .msg files, against POI's assertions

`tests/fixtures/msg/poi-truth.json` holds what Apache POI's own HSMF tests (apache/poi 81ab5262,
four test files, each SHA-256 recorded there) assert of the four committed POI `.msg` files, each
value mapped to the CLI field that holds it, written as a JSON pointer into the message line.
Besides equal values, POI asserts a body's Java `String.hashCode`, scored here over the CLI's
field's UTF-16 code units (the "hash" rows), and a recipient list's length (the "count" rows).
Assertions by prefix or containment, on the RTF body, on Exchange recipients' SMTP addresses, or
after POI's code page guessing is overridden are left out; `msgClassAppointment.msg` has no
assertions and is read only. `tests/msg.rs` checks the same file.

| Field kind | Expected | Recovered | Mismatched | Missing |
|---|---|---|---|---|
| Class | 1 | 1 | 0 | 0 |
| Subject | 3 | 3 | 0 | 0 |
| Conversation topic | 1 | 1 | 0 | 0 |
| Sender name | 2 | 2 | 0 | 0 |
| To name | 3 | 3 | 0 | 0 |
| To address | 3 | 3 | 0 | 0 |
| To count | 2 | 2 | 0 | 0 |
| Cc name | 3 | 3 | 0 | 0 |
| Cc address | 3 | 3 | 0 | 0 |
| Cc count | 1 | 1 | 0 | 0 |
| Bcc name | 2 | 2 | 0 | 0 |
| Bcc address | 2 | 2 | 0 | 0 |
| Bcc count | 1 | 1 | 0 | 0 |
| Body, plain text | 1 | 1 | 0 | 0 |
| Body, plain text (hash) | 1 | 1 | 0 | 0 |
| Body, HTML | 1 | 1 | 0 | 0 |
| Body, HTML (hash) | 1 | 1 | 0 | 0 |
| **All** | **31** | **31** | **0** | **0** |

`ASCII_UTF-8_CP1252_LCID1031_HTML.msg` holds its subject and plain text in its locale's code page,
1252 (locale 1031, German), while its internet code page is UTF-8, and its HTML body in UTF-8:
the subject and plain text match POI's assertions by the locale step of the README's fallback,
which skips a Unicode internet code page, and the HTML by its internet code page.

## Time and peak memory

Each run started through `bench/rusage.c`, office's wrapper, which forks the CLI and reports its
own `wait4` rusage: peak resident memory (`ru_maxrss`) and wall time from fork to exit, the
process's start and exit included. The wrapper's floor, `/bin/true` through it, was 1,528 KiB and
0.68 ms. stdout and stderr went to files. plaso's store and the scale store ran 3 times each way,
back to back; each committed store once each way. Every store's stdout was byte-identical across
its runs and with and without `--attachments`.

### plaso's store and the scale store

| Store | Size | `--attachments` | Median wall, of 3 | Runs | Peak memory, most of 3 | Lines out | Written |
|---|---|---|---|---|---|---|---|
| plaso's `nromanoff@stark-research-labs.com.pst` | 59,442,176 B | no | 1.592 s | 1.592, 1.601, 1.580 s | 41.6 MiB | 18 folders, 540 messages, 3 skipped | |
| plaso's | | yes | 1.604 s | 1.593, 1.615, 1.604 s | 41.3 MiB | the same | 5 files, 14,533,959 B |
| scale store | 2,141,594,624 B | no | 13.267 s | 13.267, 13.277, 13.112 s | 65.1 MiB | 17 folders, 50,000 messages | |
| scale store | | yes | 14.385 s | 14.471, 14.385, 14.349 s | 67.0 MiB | the same | 21,202 files, 1,877,202,474 B |

CPU time, per run: plaso's about 1.3 s user and 0.23 s system; the scale store's about 9.9 s user
and 3.2 s system, 10.1 s and 4.0 s with `--attachments`. User and system time add up to the wall
time: one core does the work, reading the store in order.

**plaso's store** is `test_data/nromanoff@stark-research-labs.com.pst` from
[log2timeline/plaso](https://github.com/log2timeline/plaso) (Apache-2.0) at f5bf35f, the commit
that added it, SHA-256 `bab64f3589f8ce758df9e595b89a5e4b9be159929e49100397152fdcc7bd6013`. The run
fetches it once into `.state/findings/plaso/` and checks it against that hash on every run; it
is never committed. It is a real Outlook Unicode store: 18 folders, 539 `IPM.Note` messages
and 1 `IPM.Contact`, and 6 attachment records (5 distinct contents, 14.5 MB; one spreadsheet is
attached twice). Three items of its Suggested
Contacts folder are skipped, each with a warning: outlook-pst 1.2.0 can't open them
(`Missing Sub-Node Tree on message`), as with java-libpst's `dist-list.pst`.

**The scale store** is generated, never committed: `MISE_ENV=fixtures mise run scale`, seed 16,
50,000 messages (`generator/Scale.cs`; its parameters and counts are in the results file's
`scale.record`):

| Parameter | Value |
|---|---|
| Seed | 16 |
| Messages | 50,000, each MIME the generator writes, added through PstBuilder.Eml 1.3.0 |
| Folders | 12 by weight, three deep, one non-ASCII (`Inbox/Projects/Été`); 17 with their parents, the root, and PST Builder's own Deleted Items |
| People | 200, names from Latin, Cyrillic, and CJK lists |
| Bodies | plain only 30%, plain and HTML 60%, HTML only 10%; 1 to 6 paragraphs of 1 to 5 sentences |
| Replies | 30%, with In-Reply-To and References (PST Builder doesn't store them) |
| Inline images | 10% of the HTML messages, 2 to 32 KiB, by `cid:` |
| Attachments | 18% of messages carry 1 to 3, of eight types, log-uniform from 256 B to 512 KiB; 0.2% carry one more of 1 to 16 MiB |
| Built | 34,889 with HTML (4,946 HTML only), 14,977 replies, 3,374 inline images, 17,828 attachments of 1,818,207,086 B, the largest 16,736,336 B; 2,690,853,493 B of MIME |
| Tools | .NET SDK 10.0.402 (runtime 10.0.13), PstBuilder and PstBuilder.Eml 1.3.0, MimeKit 4.18.2 |
| Content SHA-256 | `d40792a45510dc0d4b7a3abc527848095d4f255bae26d853ee7f9a87119362b4`, over every message's folder and MIME bytes in order |

The store's bytes differ on every build (PST Builder's random store UID and search keys), so its
content is what is pinned: three full builds of seed 16 before the run, about 33 s each, gave the
same content SHA-256 and the same 2,141,594,624-byte size, and the CLI's stdout on the last two
was byte-identical (SHA-256 `441b5c31...c04b6b`, the run's own stdout too); two 500-message
builds also gave byte-identical CLI output. The CLI read every message: no record skipped, no
warning, and with `--attachments` one file for each of the 17,828 attachments and 3,374 inline
images (21,202, every content distinct).

### The release binary against a glibc build

The release binary is static musl. The same commit built for the host's glibc target
(`cargo build --release`, never released) was timed beside it, without `--attachments`:

| Store | musl (released): median wall | peak | glibc: median wall | peak |
|---|---|---|---|---|
| plaso's | 1.592 s | 41.6 MiB | 0.707 s | 46.6 MiB |
| scale store | 13.267 s | 65.1 MiB | 6.510 s | 74.3 MiB |

musl's build takes 2.0 (scale store) to 2.3 (plaso's) times as long: on the scale store 9.9 s of
user time against 5.8 s, and 3.2 s of system time against 0.7 s, which points at musl's allocator
rather than the reading; the run doesn't profile it. It uses 11 to 12% less memory.

### The committed stores

One run each way; each is under 8 ms and 3 MiB, mostly the process's own start:

| Store | Size | Wall, without and with `--attachments` | Peak memory | Messages, folders, skipped | Warnings | Written with `--attachments` |
|---|---|---|---|---|---|---|
| `generated/pst/mailbox.pst` | 271,360 B | 2.6, 2.8 ms | 2.4, 2.4 MiB | 8, 9, 0 | 0 | 3 files, 137 B |
| `pst/tika/testPST.pst` | 2,302,976 B | 7.1, 6.6 ms | 2.4, 2.6 MiB | 7, 2, 0 | 1: an embedded message, listed without content | none |
| `pst/tika/testPST_variousBodyTypes.pst` | 271,360 B | 5.3, 5.4 ms | 2.6, 2.6 MiB | 4, 4, 0 | 0 | none |
| `pst/libyal/outlook.pst` | 271,360 B | 5.5, 6.0 ms | 2.6, 2.8 MiB | 2, 17, 0 | 0 | 11 files, 21,871 B |
| `pst/java-libpst/dist-list.pst` | 271,360 B | 2.2, 2.3 ms | 2.0, 2.4 MiB | 2, 13, 1 | 3: two embedded messages, one contact skipped | none |
| `pst/java-libpst/passworded.pst` | 271,360 B | 1.7, 1.7 ms | 2.0, 2.0 MiB | 1, 13, 1 | 1: one contact skipped | none |

## .eml and mbox

Every number in this section comes from run
[`mail-cli/runs/v0.2.0/results.json`](mail-cli/runs/v0.2.0/results.json), on the same laptop on
2026-11-04 from 12:03:06 to 12:06:14 UTC, at commit b3f2f8b (`v0.1.0-8-gb3f2f8b`), the static musl
release binary for x86_64 (4,101,688 bytes, SHA-256 `4fa7572d...6cdd0c7`). The run is named for
v0.2.0, the release that adds .eml and mbox, and was made before the version was raised for it, so
the binary still prints `0.1.0`; the code that reads .eml and mbox is the release's. The run also
repeats every table above: its .pst and .msg scores are v0.1.0's exactly, and of its times only
scale.pst's are cited here, beside the mboxrd's.

### Fields recovered per kind

**The .eml sources, against their hand truth.** The eight generated `.eml` against
`tests/fixtures/generated/truth.json`, and the MIME group's `09-threading.eml` against
`tests/fixtures/mime/truth.json`, scored as the generated files are above, with one difference:
an `.eml` is the source itself, so no value is a generator gap. 09's truth adds the threading and
priority fields, scored where the truth has them.

| Field kind | The eight: expected | recovered | mismatched or missing | 09-threading: expected | recovered | mismatched or missing |
|---|---|---|---|---|---|---|
| Message | 8 | 8 | 0 | 1 | 1 | 0 |
| Subject | 8 | 8 | 0 | 1 | 1 | 0 |
| Sender | 8 | 8 | 0 | 1 | 1 | 0 |
| Recipient (To) | 9 | 9 | 0 | 1 | 1 | 0 |
| Recipient (Cc) | 2 | 2 | 0 | 1 | 1 | 0 |
| Recipient (Bcc) | 1 | 1 | 0 | | | |
| Submit time | 8 | 8 | 0 | 1 | 1 | 0 |
| Delivery time | 6 | 6 | 0 | 1 | 1 | 0 |
| Importance | 1 | 1 | 0 | 1 | 1 | 0 |
| Message-ID | 8 | 8 | 0 | 1 | 1 | 0 |
| In-Reply-To | 1 | 1 | 0 | 1 | 1 | 0 |
| References | 2 | 2 | 0 | 1 | 1 | 0 |
| Conversation topic | | | | 1 | 1 | 0 |
| Conversation index | | | | 1 | 1 | 0 |
| Sensitivity | | | | 1 | 1 | 0 |
| Body, plain text | 7 | 7 | 0 | 1 | 1 | 0 |
| Body, HTML | 3 | 3 | 0 | | | |
| Attachment content (SHA-256 and size) | 3 | 3 | 0 | | | |
| Attachment filename | 3 | 3 | 0 | | | |
| Attachment MIME type | 3 | 3 | 0 | | | |
| Attachment content-id | 1 | 1 | 0 | | | |
| Attachment inline flag | 3 | 3 | 0 | | | |
| Embedded message | 1 | 1 | 0 | | | |
| **All** | **86** | **86** | **0** | **15** | **15** | **0** |

Every field the .pst and .msg lose to their generators is recovered from the .eml: Bcc,
importance, the threading fields, and the embedded message, read in full.

**The mboxes, against their sources.** `tests/fixtures/mime/nine.mboxrd` holds the nine `.eml`
above (scripts/mboxrd.sh); each of its messages is scored against the CLI's read of its source
`.eml`. Apache James mime4j's `mbox.rlug` has five expected splits; each message is scored against
the CLI's read of its split as an `.eml`, without the split's last line ending (the blank line the
reader gives the separator, as `tests/mbox.rs` says). A message is found by its place in the file,
its id scored as its ordinal; then every field of the source's message line is a kind, a body's
parts each their own, scored for being equal. A field the mbox's message has and its source's
hasn't would count as unexpected; there were none.

| Field kind | nine.mboxrd: expected | recovered | mismatched or missing | mbox.rlug: expected | recovered | mismatched or missing |
|---|---|---|---|---|---|---|
| Message | 9 | 9 | 0 | 5 | 5 | 0 |
| Id (the ordinal) | 9 | 9 | 0 | 5 | 5 | 0 |
| Subject | 9 | 9 | 0 | 5 | 5 | 0 |
| Sender | 9 | 9 | 0 | 5 | 5 | 0 |
| To | 9 | 9 | 0 | 5 | 5 | 0 |
| Cc | 3 | 3 | 0 | | | |
| Bcc | 1 | 1 | 0 | | | |
| Submit time | 9 | 9 | 0 | 5 | 5 | 0 |
| Delivery time | 7 | 7 | 0 | 5 | 5 | 0 |
| Importance | 2 | 2 | 0 | 1 | 1 | 0 |
| Sensitivity | 1 | 1 | 0 | | | |
| Message-ID | 9 | 9 | 0 | 5 | 5 | 0 |
| In-Reply-To | 2 | 2 | 0 | 1 | 1 | 0 |
| References | 3 | 3 | 0 | 2 | 2 | 0 |
| Conversation topic | 1 | 1 | 0 | | | |
| Conversation index | 1 | 1 | 0 | | | |
| Transport headers | 9 | 9 | 0 | 5 | 5 | 0 |
| Body, text | 8 | 8 | 0 | 5 | 5 | 0 |
| Body, HTML | 3 | 3 | 0 | | | |
| Body, derived | 9 | 9 | 0 | 5 | 5 | 0 |
| Attachments (the whole list, per message) | 3 | 3 | 0 | | | |
| **All** | **116** | **116** | **0** | **59** | **59** | **0** |

`mbox.rlug` is 7-bit ASCII with no attachments, so it tests the splitting, not decoding; the
nine.mboxrd's sources carry the charsets, attachments and the embedded message, and 09's body
lines starting `From ` and `>From `, quoted in the file and read back as they were.

### Time and peak memory: the scale mboxrd beside the scale store

The scale mode writes the same 50,000 messages, the same MIME in the same order, as
`.state/scale/scale.mboxrd` beside `scale.pst`, by scripts/mboxrd.sh's conventions: the fixed
separator `From MAILER-DAEMON Thu Jan  1 00:00:00 1970` before each message, one more `>` on each
line matching `^>*From ` (none of these messages has one), and a blank line after each. Built from
seed 16 just before the run, in 36.9 s with the .pst: content SHA-256
`d40792a45510dc0d4b7a3abc527848095d4f255bae26d853ee7f9a87119362b4` and a 2,141,594,624-byte
`scale.pst`, both as at v0.1.0, and a 2,693,103,493-byte mboxrd, SHA-256
`79f5ad34bd4962a1bd84974fd1b4e06bd06bbba7718e5db1321ea64dbb22fe1a` (its bytes, unlike the .pst's,
are a function of the messages, so `scale.json` records its hash). Measured as above: through
`bench/rusage.c` (floor 1,528 KiB, 0.67 ms), 3 runs each way back to back, warm.

| Store | Size | `--attachments` | Median wall, of 3 | Runs | Peak memory, most of 3 | Lines out | Written |
|---|---|---|---|---|---|---|---|
| scale.mboxrd | 2,693,103,493 B | no | 6.858 s | 6.858, 6.984, 6.850 s | 41.0 MiB | 50,000 messages | |
| scale.mboxrd | | yes | 7.611 s | 7.534, 7.611, 7.679 s | 42.2 MiB | the same | 21,202 files, 1,877,202,474 B |
| scale.pst, the same run | 2,141,594,624 B | no | 14.818 s | 15.671, 14.686, 14.818 s | 65.9 MiB | 17 folders, 50,000 messages | |
| scale.pst, the same run | | yes | 16.065 s | 16.410, 15.913, 16.065 s | 66.9 MiB | the same | 21,202 files, 1,877,202,474 B |

The mboxrd is read in 0.46 of the .pst's time (0.47 with `--attachments`) at 62 to 63% of its
peak memory, though it is 26% larger: about 4.0 s user and 2.7 s system per run (4.2 s and 3.2 s
with `--attachments`), against the .pst's 9.7 to 10.1 s and 3.8 to 4.3 s. Peak memory follows the
message being read: 41 MiB for 2.7 GB, close to plaso's 59 MB store's 41.6 MiB. No record was
skipped and no warning written; every stdout was byte-identical across its runs and with and
without `--attachments`, and `--attachments` wrote as many files of as many bytes from both, 21,202 of 1,877,202,474 B. The .pst's
stdout had the SHA-256 it had at v0.1.0 (`441b5c31...c04b6b`). The .pst's medians are 12% above
v0.1.0's on the same store (14.82 s against 13.27 s), the spread between two days' runs on this
laptop, so the mboxrd is compared only with the .pst of its own run.

The glibc build, timed beside it as above, without `--attachments`:

| Store | musl (released): median wall | peak | glibc: median wall | peak |
|---|---|---|---|---|
| scale.mboxrd | 6.858 s | 41.0 MiB | 4.094 s | 61.3 MiB |
| scale.pst, the same run | 14.818 s | 65.9 MiB | 6.877 s | 74.1 MiB |

musl takes 1.7 times glibc's time on the mboxrd, less than on the .pst (2.2 here), and a third
less memory.

## Known limits

The CLI's, as its README lists them, and what this run shows of each:

- **Embedded messages in a `.pst` are listed without content** (outlook-pst 1.2.0 can't read
  them; the fix is unmerged upstream), with a warning each: `testPST.pst`'s one and
  `dist-list.pst`'s two; the generated stores have none. In a `.msg` they are read in full.
- **ANSI stores are read but untested**: no store here is ANSI.
- **`.ost` is refused**, tested on a header built in the test; none was measured.
- **By-reference attachments have no file**; none of these stores has one.
- **Class-specific fields are absent**: plaso's contact is written with the message fields alone.
- **A code page that can't be decoded is decoded lossily**, with a warning; no store here needed it.
- **An Exchange address is written as stored**, its SMTP address not at all: Tika's
  `testMSG_chinese.msg` To address, above.
- **An item outlook-pst 1.2.0 can't open is skipped**: plaso's three Suggested Contacts and
  the one contact of each of java-libpst's stores.
- **A folder's table that can't be read is told from an empty one only by the folder's count**
  (outlook-pst 1.2.0 gives an unreadable table as no table, without its error), so the skipped
  line's reason can't name the fault, and a folder claiming no items or subfolders while it has
  some loses them silently. No store here has an unreadable folder table: every skipped line in
  this run is a message. It is tested on a copy of libyal's store with one byte changed.

The finding has limits of its own: the times are warm (the page cache held every store); the scale store
is synthetic, made by one generator whose gaps (no threading fields, no embedded messages, always
normal importance) it shares with the generated fixtures; and only plaso's store is a real store
of any size.

## Rerunning

From this repository's root, on a laptop (never in CI, never in the check):

```sh
MISE_ENV=fixtures mise run scale   # once: .state/scale/scale.pst, scale.mboxrd and scale.json, about 37 s
mise run findings
```

`mise run findings` (`scripts/findings.sh`) fetches plaso's store into `.state/findings/plaso/`
at the pinned commit and refuses it unless its SHA-256 is the pinned one, builds the release
binary, the glibc build, the scorer (`bench/findings.rs`, a Cargo example the check compiles and
never runs), and `bench/rusage.c` (with `cc`), then scores the fixtures and times every store,
printing the tables. A plain run keeps nothing: it writes `.state/findings/results.json`, which
the next run overwrites, and `.state/` is ignored. To keep a run, record it:

```sh
mise run findings -- --record [name]
```

That copies the results to `docs/findings/mail-cli/runs/<name>/results.json`, `<name>` by default
`<date>-<n>`, the first such directory that doesn't exist; a name is letters, digits, `.`, `_`,
and `-`, not starting with `.`, and an existing run directory is never overwritten. Run
`v0.1.0`'s scoring and timing took 116 s, after the builds; run `v0.2.0`'s, with the mboxrd, 188 s. `FINDINGS_RUNS` (3) sets the runs of
plaso's store and the scale store; the generator's `scale` command takes `--seed <n>` and
`--messages <n>` after `--`.

In `results.json` (schema 1): `cli` is the binary's version, commit, SHA-256, and size; `machine`
the CPU, logical CPUs, memory, and kernel; `recovery` holds, for each table (`generated_pst`,
`generated_msg`, `tika_pst_threading`, `tika_msg`, `poi_msg`, and from v0.2.0 `generated_eml`,
`mime_eml`, `nine_mboxrd`, `rlug_mbox`), its files, each kind's counts, and
every miss with its file, message, kind, outcome, and expected and written values; `rusage_floor` the
wrapper's own; `plaso` the store's source and SHA-256; `scale` the store's size, from v0.2.0 the mboxrd's size and
SHA-256, and its `scale.json`; and `timing` each store's runs (status, exit code, peak memory, wall, user, and
system time), median wall, peak, stdout size, whether stdout was the same every run, its lines by
kind, its attachment records, its warnings, and what `--attachments` wrote.
