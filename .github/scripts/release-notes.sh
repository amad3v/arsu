#!/usr/bin/env bash
# Prints a release's notes: the body of its `## [VERSION]` section in
# CHANGELOG.md, up to the next version's heading. Fails if the section is
# missing or empty, so a release cannot go out without its notes.
#
# Usage: .github/scripts/release-notes.sh 1.1.0 [path/to/CHANGELOG.md]
set -euo pipefail

version=${1:?usage: release-notes.sh VERSION [CHANGELOG]}
changelog=${2:-"$(dirname "$0")/../../CHANGELOG.md"}

notes=$(awk -v heading="## [$version]" '
  index($0, heading) == 1 { found = 1; next }
  found && /^## \[/ { exit }
  found { print }
' "$changelog" | sed -e '/./,$!d')

if [[ -z "${notes//[[:space:]]/}" ]]; then
  echo "error: CHANGELOG.md has no notes under '## [$version]'" >&2
  exit 1
fi
printf '%s\n' "$notes"
