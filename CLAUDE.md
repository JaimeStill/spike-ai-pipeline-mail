# spike-ai-pipeline-mail

The mail CLI of spike-ai-pipeline. The repository is managed with the marathon workflow; start
from `context/README.md`.

- **Standards:** `STANDARDS.md`, with pointers into the architecture repository.
- **Layout:** one Cargo crate; the CLI in `src/`, its tests in `tests/`, run against the built
  binary.
- **Check:** `mise run check`: shellcheck, `cargo fmt --check`, `cargo clippy` with warnings
  denied, `cargo test --locked`, and a static musl release build for this machine.
- **Currency:** `mise run currency`.
- **Goals:** the spike's tasks live in the coordinator's roadmap,
  `standards-lab/context/roadmap.toml`, under `experiment.ai.spike-ai-pipeline`. Its goal record
  is in the root repository, `spike-ai-pipeline`, at
  `context/goals/experiment.ai.spike-ai-pipeline.md`.
- **Dependencies:** crates at published versions, locked in `Cargo.lock`; Rust pinned in mise.
- **References:** the repositories this spike reads are keys in the coordinator's
  `references.toml` and `references.local.toml`. The spike reads them and never writes to them.
