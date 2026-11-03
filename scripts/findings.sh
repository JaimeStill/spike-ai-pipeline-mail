#!/usr/bin/env bash
# findings runs the CLI's finding for evidence 16 on this machine, never in CI and never in the
# check: the fields the CLI recovers per field kind against the committed fixtures' truth, and its
# time and peak memory on every committed .pst, on plaso's 59 MB store, and on the scale store,
# each with and without --attachments. Run it through `mise run findings`, which passes its
# arguments here:
#
#   mise run findings                     keeps nothing: the results go to
#                                         .state/findings/results.json, overwritten each run, and
#                                         the tables to the terminal
#   mise run findings -- --record [name]  records the run in docs/findings/mail-cli/runs/<name>/
#                                         results.json, <name> by default <date>-<n>; an existing
#                                         run directory is never overwritten
#
# A name is letters, digits, '.', '_', and '-', and doesn't start with '.'. It needs:
# - the scale store, .state/scale/scale.pst and its scale.json, which
#   `MISE_ENV=fixtures mise run scale` builds (about 2 GB; never committed);
# - plaso's store, which it fetches into .state/findings/ at a pinned commit and refuses unless
#   its SHA-256 is the pinned one (never committed);
# - cc, to build bench/rusage.c, the wrapper that measures each run.
# FINDINGS_RUNS (3) sets the runs of plaso's and the scale store; every other store runs once.
set -euo pipefail
cd "$MISE_PROJECT_ROOT"

usage() {
	echo "usage: mise run findings [-- --record [name]]" >&2
	exit 2
}

runs_dir=docs/findings/mail-cli/runs
state=.state/findings
record=""
case "$#" in
0) ;;
1 | 2)
	[[ $1 == --record ]] || usage
	if [[ $# -eq 2 ]]; then
		record=$2
		if [[ ! $record =~ ^[A-Za-z0-9_-][A-Za-z0-9._-]*$ ]]; then
			echo "findings: run name '$record' isn't letters, digits, '.', '_', and '-', not starting with '.'" >&2
			exit 2
		fi
	else
		# The first <date>-<n> that no run directory has.
		day=$(date +%F)
		n=1
		while [[ -e $runs_dir/$day-$n ]]; do
			n=$((n + 1))
		done
		record=$day-$n
	fi
	if [[ -e $runs_dir/$record ]]; then
		echo "findings: $runs_dir/$record exists; a recorded run is never overwritten" >&2
		exit 1
	fi
	;;
*) usage ;;
esac

scale=.state/scale
if [[ ! -f $scale/scale.pst || ! -f $scale/scale.json ]]; then
	echo "findings: no scale store in $scale; build it first: MISE_ENV=fixtures mise run scale" >&2
	exit 1
fi

# plaso's store, at the commit that added it, by its SHA-256.
plaso_commit=f5bf35fcc102585eed58b930392da56a318690f6
plaso_sha256=bab64f3589f8ce758df9e595b89a5e4b9be159929e49100397152fdcc7bd6013
plaso_url="https://raw.githubusercontent.com/log2timeline/plaso/$plaso_commit/test_data/nromanoff%40stark-research-labs.com.pst"
plaso="$state/plaso/nromanoff@stark-research-labs.com.pst"
mkdir -p "$state/plaso"
if [[ ! -f $plaso ]] || ! echo "$plaso_sha256  $plaso" | sha256sum --check --status; then
	echo "findings: fetching plaso's store at $plaso_commit" >&2
	curl --fail --silent --show-error --location --output "$plaso.part" "$plaso_url"
	mv "$plaso.part" "$plaso"
fi
if ! echo "$plaso_sha256  $plaso" | sha256sum --check --status; then
	echo "findings: $plaso isn't plaso's store: its SHA-256 isn't $plaso_sha256" >&2
	rm -f "$plaso"
	exit 1
fi

# The release build the check makes; the host's glibc build, timed beside it on the two large
# stores; and the scorer and the wrapper.
target="$(uname -m)-unknown-linux-musl"
cargo build --release --locked --target "$target"
cargo build --release --locked
cargo build --release --locked --example findings
cc -O2 -Wall -Werror -o "$state/rusage" bench/rusage.c

commit=$(git describe --always --dirty)
"target/release/examples/findings" \
	--bin "target/$target/release/spike-ai-pipeline-mail" \
	--glibc-bin target/release/spike-ai-pipeline-mail \
	--rusage "$state/rusage" \
	--plaso "$plaso" \
	--plaso-commit "$plaso_commit" \
	--scale "$scale" \
	--state "$state" \
	--out "$state/results.json" \
	--commit "$commit" \
	--runs "${FINDINGS_RUNS:-3}"

if [[ -n $record ]]; then
	mkdir -p "$runs_dir/$record"
	cp "$state/results.json" "$runs_dir/$record/results.json"
	echo "findings: recorded $runs_dir/$record/results.json" >&2
fi
