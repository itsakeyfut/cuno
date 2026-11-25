#!/usr/bin/env bash
# Tests for the release scripts, run by CI on every pull request, so what
# decides a release is proven before a release depends on it.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
failed=0

expect() {
    local what="$1" want="$2" got="$3"
    if [ "$want" = "$got" ]; then
        echo "ok    $what"
    else
        echo "FAIL  $what"
        echo "      want: $(printf '%q' "$want")"
        echo "      got:  $(printf '%q' "$got")"
        failed=1
    fi
}

version() {
    out="$("$here/release-version.sh" "$@" 2>/dev/null)"
    echo "$out:$?"
}

expect "the first release is released" "release:0" "$(version 0.1.0 '')"
expect "an unchanged version is skipped" "skip:0" "$(version 0.1.0 0.1.0)"
expect "a higher version is released" "release:0" "$(version 0.2.0 0.1.0)"
expect "versions compare as numbers" "release:0" "$(version 0.10.0 0.9.0)"
expect "a lower version fails" ":1" "$(version 0.1.0 0.2.0)"

changelog="$(mktemp)"
trap 'rm -f "$changelog"' EXIT
cat > "$changelog" <<'EOF'
# Changelog

## [Unreleased]

## [0.2.0] - 2026-12-01

### Added

- Two.

## [0.1.0] - 2026-11-30

- One.

## [0.0.9] - 2026-11-01

EOF

section() {
    out="$("$here/changelog-section.sh" "$1" "$changelog" 2>/dev/null)"
    echo "$out:$?"
}

expect "a section is printed whole and alone" "$(printf '### Added\n\n- Two.'):0" "$(section 0.2.0)"
expect "the last section stops at the next heading" "- One.:0" "$(section 0.1.0)"
expect "a missing section fails" ":1" "$(section 0.3.0)"
expect "an empty section fails" ":1" "$(section 0.0.9)"
expect "a version is not matched by its prefix" ":1" "$(section 0.1)"

exit "$failed"
