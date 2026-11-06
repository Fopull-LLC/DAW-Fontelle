#!/usr/bin/env bash
# The body of a release's GitHub page: release-body.sh <tag> <version> <owner/repo>
#
# The Download line first (hub card 0620: somebody landing here from a
# search is sent to the site), then the release's own notes from
# release-notes/<tag>.md (hub card 0665: three releases went out saying
# nothing, and the app's "What's new" opens these on fopull.com), then
# Steinberg's logo and words for the ASIO helper (docs/asio.md §1).
# A tag with no notes is refused rather than published empty.
set -euo pipefail
tag="$1"
version="$2"
repo="$3"
notes="release-notes/$tag.md"
if [ ! -s "$notes" ]; then
  echo "$notes is missing or empty: write the release notes before tagging" >&2
  exit 1
fi
printf 'Download: https://fopull.com/fontelle\n\n'
# Line ends as written, whatever the checkout did to them.
tr -d '\r' < "$notes" | sed -e '$a\'
printf '\n---\n\n'
printf '<img src="https://raw.githubusercontent.com/%s/%s/assets/trademarks/asio/ASIO-compatible-logo-Steinberg-TM-BW.png" alt="ASIO Compatible Logo" width="110"/>\n\n' "$repo" "$tag"
printf 'The Windows download can play through ASIO® drivers by way of `fontelle-asio.exe`, a separate GPLv3 program whose source is attached below as `fontelle-asio-source-%s.tar.gz`. ASIO is a registered trademark of Steinberg Media Technologies GmbH.\n' "$version"
