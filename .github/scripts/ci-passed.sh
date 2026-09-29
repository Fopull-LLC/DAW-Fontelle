#!/usr/bin/env bash
# Waits for the CI workflow's run on one commit, and succeeds only if it
# passed. The release workflow's gate: nothing is published from a commit CI
# has not passed.
#
# Why it exists: CI failed on every push from v0.8.0 (2026-09-17) to v0.17.0
# (2026-09-26) and ten releases shipped anyway, because the release workflow
# never looked. Two of those failures were real bugs — an allocation on the
# audio thread on Windows, and captions that overflowed on any Linux machine
# without Open Sans.
#
#   ci-passed.sh <full commit sha> [owner/repo]
#
# Needs `gh` and a token that can read Actions (GH_TOKEN in a workflow).
# POLL and TRIES are overridable so this can be run by hand.
set -euo pipefail

sha="${1:?usage: ci-passed.sh <full commit sha> [owner/repo]}"
repo="${2:-${GITHUB_REPOSITORY:-}}"
poll="${POLL:-30}"
# CI takes about twenty minutes; an hour covers a slow runner queue.
tries="${TRIES:-120}"
# How long a commit may go without any CI run before that is an answer: a tag
# on a commit that was never pushed to main has no run and never will.
grace="${GRACE:-10}"

repo_args=()
if [ -n "$repo" ]; then
  repo_args=(--repo "$repo")
fi

for ((i = 1; i <= tries; i++)); do
  run=$(gh run list "${repo_args[@]}" --workflow CI --commit "$sha" --event push \
    --json databaseId,status,conclusion,url --limit 1 --jq '.[0] // empty')
  if [ -z "$run" ]; then
    if ((i >= grace)); then
      echo "::error::CI has no run for $sha. Push the commit to main (CI runs there), let it pass, then tag it."
      exit 1
    fi
    echo "no CI run for $sha yet ($i/$grace)"
  else
    status=$(jq -r .status <<<"$run")
    conclusion=$(jq -r .conclusion <<<"$run")
    url=$(jq -r .url <<<"$run")
    if [ "$status" = "completed" ]; then
      if [ "$conclusion" = "success" ]; then
        echo "CI passed on $sha: $url"
        exit 0
      fi
      echo "::error::CI $conclusion on $sha — not releasing it: $url"
      exit 1
    fi
    echo "CI is $status on $sha ($i/$tries): $url"
  fi
  sleep "$poll"
done

echo "::error::CI did not finish on $sha in time."
exit 1
