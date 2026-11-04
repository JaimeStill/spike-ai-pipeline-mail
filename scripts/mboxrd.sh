#!/usr/bin/env bash
# mboxrd writes the MIME group's mboxrd: the eight generated .eml under
# tests/fixtures/generated/eml and the group's own under tests/fixtures/mime/eml,
# in name order, so their ordinals are 1 to 9. Each message is its source byte
# for byte, but that every line matching ^>*From  gains one ">", after one fixed
# separator line; a blank line ends it. The separator line is the same for every
# message, so the output depends on the sources alone. The check rebuilds the file
# and compares it byte for byte with the committed one.
#
# usage: scripts/mboxrd.sh [output]
#   output  the file written, tests/fixtures/mime/nine.mboxrd by default
set -euo pipefail
cd "$(dirname "$0")/.."
export LC_ALL=C

if [ $# -gt 1 ]; then
	echo "usage: scripts/mboxrd.sh [output]" >&2
	exit 2
fi
output=${1:-tests/fixtures/mime/nine.mboxrd}

# The sources by file name, wherever each lives: the names start with their ordinal.
mapfile -t sources < <(
	for source in tests/fixtures/generated/eml/*.eml tests/fixtures/mime/eml/*.eml; do
		printf '%s\t%s\n' "$(basename "$source")" "$source"
	done | sort | cut -f2
)

{
	for source in "${sources[@]}"; do
		printf 'From MAILER-DAEMON Thu Jan  1 00:00:00 1970\n'
		sed -e '/^>*From /s/^/>/' "$source"
		printf '\n'
	done
} >"$output"
