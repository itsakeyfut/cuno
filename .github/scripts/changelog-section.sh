#!/usr/bin/env bash
# Prints the body of CHANGELOG.md's section for a version: the lines after
# `## [<version>]` up to the next `## ` heading, without blank lines at
# either end. Fails when there is no such section or it is empty, so a
# release is never cut without notes.
#
# Usage: changelog-section.sh <version> <CHANGELOG.md>
set -euo pipefail

version="${1:?the version}"
file="${2:?the changelog}"

body="$(awk -v v="$version" '
    index($0, "## [" v "]") == 1 { inside = 1; next }
    inside && /^## / { exit }
    inside { print }
' "$file" | sed -e '/./,$!d' | sed -e ':a' -e '/^\n*$/{$d;N;ba' -e '}')"

if [ -z "$body" ]; then
    echo "CHANGELOG.md has no section for $version, or it is empty" >&2
    exit 1
fi
printf '%s\n' "$body"
