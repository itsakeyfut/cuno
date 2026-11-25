#!/usr/bin/env bash
# Whether the workspace version is to be released: prints `release` when it is
# the first release or higher than the last released one, `skip` when it
# equals it, and fails when it is lower, so a mistaken downgrade publishes
# nothing (docs/en/specs/distribution.md, "Installation").
#
# Usage: release-version.sh <version> <last released version, or empty>
set -euo pipefail

new="${1:?the workspace version}"
last="${2:-}"

if [ -z "$last" ]; then
    echo release
elif [ "$new" = "$last" ]; then
    echo skip
elif [ "$(printf '%s\n%s\n' "$last" "$new" | sort -V | tail -1)" = "$new" ]; then
    echo release
else
    echo "version $new is lower than the last release, $last" >&2
    exit 1
fi
