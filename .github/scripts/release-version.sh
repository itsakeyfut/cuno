#!/usr/bin/env bash
# Whether the workspace version is to be released: prints `release` when it is
# the first release or higher than the last released one, `skip` when it
# equals it or is 0.0.0 (not released yet), and fails when it is lower, so a
# mistaken downgrade publishes nothing. Only `x.y.z` is a release version;
# anything else fails (docs/en/specs/distribution.md, "Versioning policy").
#
# Usage: release-version.sh <version> <last released version, or empty>
set -euo pipefail

new="${1:?the workspace version}"
last="${2:-}"
plain='^[0-9]+\.[0-9]+\.[0-9]+$'

if ! [[ "$new" =~ $plain ]]; then
    echo "version $new is not of the form x.y.z, the only form cuno releases" >&2
    exit 1
fi
if [ -n "$last" ] && ! [[ "$last" =~ $plain ]]; then
    echo "the last release, $last, is not of the form x.y.z" >&2
    exit 1
fi

if [ "$new" = 0.0.0 ] || [ "$new" = "$last" ]; then
    echo skip
elif [ -z "$last" ]; then
    echo release
else
    # Field by field, as numbers: x.y.z orders this way and no other.
    IFS=. read -r a1 a2 a3 <<< "$new"
    IFS=. read -r b1 b2 b3 <<< "$last"
    if (( a1 > b1 || (a1 == b1 && (a2 > b2 || (a2 == b2 && a3 > b3))) )); then
        echo release
    else
        echo "version $new is lower than the last release, $last" >&2
        exit 1
    fi
fi
