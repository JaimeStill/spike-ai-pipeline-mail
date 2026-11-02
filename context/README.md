# spike-ai-pipeline-mail

The mail CLI of spike-ai-pipeline, released by tag and run by spike-ai-pipeline-processing as a
system tool. This context records only working knowledge the code and the README do not express;
the spike's question and evidence live in the root repository's `context/README.md`.

## Capability map

- **stores**: Outlook `.pst` to the JSON model, through outlook-pst: the folder tree and each
  message in its folder; `.ost` refused with a reason.
- **messages**: standalone `.msg` to the same message model, through msg_parser.
- **fields**: headers, bodies, and threading fields (Message-ID, In-Reply-To, References,
  conversation topic and index) as stored; no thread grouping.
- **attachments**: written out as files named by SHA-256, embedded messages as nested messages.
- **release**: static musl binaries for linux-x64 and linux-arm64.
