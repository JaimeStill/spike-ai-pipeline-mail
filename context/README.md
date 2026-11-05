# spike-ai-pipeline-mail

The mail CLI of spike-ai-pipeline, released by tag, which spike-ai-pipeline-processing's `email`
package runs as a system tool. This context records only working knowledge
the code and the README do not express; the spike's question and evidence live in the root
repository's `context/README.md`.

## Capability map

- **stores**: Outlook `.pst` to JSON Lines of schema `"1"`, through outlook-pst: the folder tree
  and each message in its folder; `.ost` refused with a reason.
- **messages**: standalone `.msg` to the same message model, through the CLI's own MS-OXMSG
  reader over cfb, embedded messages nested in full.
- **MIME**: `.eml` messages and mbox files (read as mboxrd) to the same model, through
  mail-parser: headers mapped to the MAPI fields, text and HTML bodies, attachments by part, and
  message/rfc822 and message/global parts nested as embedded messages.
- **resume**: `--after <id>` resumes a `.pst` after a message's node id and an mbox after an
  ordinal, the whole read's tail byte for byte, the messages before it passed over unopened.
- **fields**: headers and threading fields (Message-ID, In-Reply-To, References, conversation
  topic and index) as stored; no thread grouping. A `.pst`'s and a `.msg`'s strings are decoded by
  their code page through codepage-strings, a MIME message's by its charsets through mail-parser.
- **bodies**: plain text, HTML, and RTF (decompressed) as stored; derived beside them, the HTML
  recovered from RTF that encapsulates it (MS-OXRTFEX) and a Markdown body made by htmd.
- **attachments**: written out as files named by SHA-256; embedded messages nested in a `.msg`,
  an `.eml`, or an mbox, listed without content in a `.pst` (outlook-pst 1.2.0 can't read them).
- **fixtures**: public `.pst` and `.msg` files with hand truth, and `.pst` and `.msg` files a C#
  generator builds from `.eml` sources; its scale mode builds a 2 GB store from a seed, and the
  same messages as an mboxrd. The MIME group: an `.eml` with hand truth, an mboxrd of the nine
  `.eml` that `mise run mboxrd` writes, and mime4j's mbox.rlug.
- **finding**: fields recovered per kind against the fixtures' truth, and time and peak memory
  on plaso's store and the generated 2 GB store, on the same messages as a 2.7 GB mboxrd, and on
  both resumed after their middle and their last message (`docs/findings/mail-cli.md`).
- **release**: static musl binaries for linux-x64 and linux-arm64.
