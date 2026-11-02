#!/usr/bin/env bash
# currency reports every dependency and toolchain that trails its latest
# release, one line each, and exits non-zero when it reports any. It reads the
# network and writes nothing outside generator/obj/. Run it through `mise run currency`.
#
# Each scan captures its command's output in a variable before reading it, so
# under set -e a scan command that fails fails currency instead of reporting
# nothing.
set -euo pipefail
cd "$MISE_PROJECT_ROOT"

stale=0
report() {
	echo "$1"
	stale=1
}

# Crates: updates within each requirement ("Updating"), and newer releases
# outside it ("Unchanged ... (available: ...)"). --dry-run leaves Cargo.lock
# as it is.
crates=$(cargo update --dry-run --verbose 2>&1 |
	sed -nE 's/^ *(Updating|Unchanged) +([^ ]+) +(v[^ ]+)( -> (v[^ ]+)| \(available: (v[^)]+)\))$/Cargo: \2 \3 -> \5\6/p')
while read -r line; do
	[ -n "$line" ] && report "$line"
done <<<"$crates"

# mise tools pinned below their latest version: mise.toml's, Rust among them, and with
# MISE_ENV=fixtures, mise.fixtures.toml's too, the generator's .NET SDK.
outdated=$(MISE_ENV=fixtures mise outdated --bump --local --json |
	jq -r 'to_entries[] | "\(.value.source.path | split("/") | last): \(.key) \(.value.requested) -> \(.value.bump)"')
while read -r line; do
	[ -n "$line" ] && report "$line"
done <<<"$outdated"

# The fixtures generator's NuGet packages with a newer release, run from generator/ so its
# global.json picks the SDK.
packages=$(cd generator && MISE_ENV=fixtures mise exec -- dotnet list package --outdated --format json |
	jq -r '.projects[] | .frameworks[]?.topLevelPackages[]? |
		"generator/Generator.csproj: \(.id) \(.requestedVersion) -> \(.latestVersion)"')
while read -r line; do
	[ -n "$line" ] && report "$line"
done <<<"$packages"

exit "$stale"
