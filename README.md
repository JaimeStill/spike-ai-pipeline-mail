# spike-ai-pipeline-mail

The mail tool of [spike-ai-pipeline](https://github.com/JaimeStill/spike-ai-pipeline): a
command-line utility that reads Outlook `.pst` stores, through Microsoft's
[outlook-pst](https://github.com/microsoft/outlook-pst-rs), and `.msg` messages, through
[msg_parser](https://crates.io/crates/msg_parser), into one JSON model: the store's folder tree,
each message in the folder it is stored in, with its headers, bodies, and threading fields as
stored, and its attachments written out as files. It builds no threads; processing does.

It is published as one static musl binary each for linux-x64 and linux-arm64, and
spike-ai-pipeline-processing runs it like any other system tool. Outlook `.ost` files, the
offline cache of a server mailbox, are refused: export the mailbox to `.pst`.

It is released by tag, `vX.Y.Z`, with a CHANGELOG. The spike's question, evidence, and answer are
in the root repository's README.

Apache-2.0.
