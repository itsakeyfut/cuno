#!/usr/bin/env bash
# Whether main's version is above the last release: its x.y.z part, with any
# -dev dropped, must be greater, so main never carries a version already
# published. Fails, saying what to do, when it is equal or lower
# (docs/en/specs/distribution.md, "Versioning policy").
#
# Usage: dev-version.sh <version> <last released version, or empty>
set -euo pipefail

version="${1:?the workspace version}"
last="${2:-}"
base="${version%-dev}"

if ! [[ "$base" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "main's version $version is neither x.y.z nor x.y.z-dev" >&2
    exit 1
fi
[ -z "$last" ] && exit 0

IFS=. read -r a1 a2 a3 <<< "$base"
IFS=. read -r b1 b2 b3 <<< "$last"
# Base 10 always: a field such as 08 is not octal.
a1=$((10#$a1)) a2=$((10#$a2)) a3=$((10#$a3))
b1=$((10#$b1)) b2=$((10#$b2)) b3=$((10#$b3))
if (( a1 > b1 || (a1 == b1 && (a2 > b2 || (a2 == b2 && a3 > b3))) )); then
    exit 0
fi
echo "main's version $version is not above the last release, $last; raise it to the next version's -dev" >&2
exit 1
