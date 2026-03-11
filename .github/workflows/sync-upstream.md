---
on:
  workflow_dispatch:
  schedule: weekly on monday

description: "Keep this fork aligned with nix-community/lanzaboote:master without merge commits."

permissions:
  contents: read
  pull-requests: read

env:
  ANTHROPIC_BASE_URL: https://api.z.ai/api/anthropic
  API_TIMEOUT_MS: "3000000"

engine:
  id: claude

network:
  allowed:
    - defaults

checkout:
  fetch-depth: 0
  fetch:
    - "*"

concurrency:
  group: sync-upstream-master
  cancel-in-progress: false

steps:
  - name: Configure git identity
    run: |
      git config user.name "github-actions[bot]"
      git config user.email "41898282+github-actions[bot]@users.noreply.github.com"

  - name: Analyze upstream sync plan
    run: |
      set -euo pipefail

      mkdir -p /tmp/gh-aw
      rm -f /tmp/gh-aw/upstream-sync.env /tmp/gh-aw/upstream-sync-report.md

      status="noop"
      reason="origin/master already matches upstream/master."
      sync_branch="sync/upstream-master"
      base_branch="master"
      upstream_repo="nix-community/lanzaboote"
      upstream_ref="upstream/master"
      origin_ref="origin/master"
      conflict_files=""

      if ! git remote get-url upstream >/dev/null 2>&1; then
        git remote add upstream "https://github.com/${upstream_repo}.git"
      fi

      git fetch --no-tags origin master
      git fetch --no-tags upstream master

      upstream_sha="$(git rev-parse "${upstream_ref}")"
      origin_sha="$(git rev-parse "${origin_ref}")"
      ahead="$(git rev-list --count "${upstream_ref}..${origin_ref}")"
      behind="$(git rev-list --count "${origin_ref}..${upstream_ref}")"

      if [ "${origin_sha}" = "${upstream_sha}" ]; then
        status="noop"
        reason="No sync needed; both branches already point at ${upstream_sha}."
      elif git merge-base --is-ancestor "${origin_ref}" "${upstream_ref}"; then
        status="fast-forward"
        reason="master can be fast-forwarded directly to upstream/master."
      else
        git checkout -B upstream-sync-analysis "${origin_ref}"
        if git rebase "${upstream_ref}"; then
          status="rebase"
          reason="Local-only commits can be rebased onto upstream/master without conflicts."
        else
          status="conflict"
          conflict_files="$(git diff --name-only --diff-filter=U | tr '\n' ',' | sed 's/,$//')"
          reason="Rebasing local-only commits onto upstream/master produced conflicts."
          git rebase --abort
        fi
        git checkout --detach "${origin_ref}"
      fi

      cat > /tmp/gh-aw/upstream-sync.env <<EOF
      SYNC_STATUS=${status}
      SYNC_REASON=${reason}
      SYNC_BRANCH=${sync_branch}
      BASE_BRANCH=${base_branch}
      UPSTREAM_REPO=${upstream_repo}
      UPSTREAM_REF=${upstream_ref}
      ORIGIN_REF=${origin_ref}
      UPSTREAM_SHA=${upstream_sha}
      ORIGIN_SHA=${origin_sha}
      AHEAD_COUNT=${ahead}
      BEHIND_COUNT=${behind}
      CONFLICT_FILES=${conflict_files}
      EOF

      cat > /tmp/gh-aw/upstream-sync-report.md <<EOF
      # Upstream Sync Status

      - Status: ${status}
      - Reason: ${reason}
      - Upstream repository: ${upstream_repo}
      - Base branch: ${base_branch}
      - Upstream SHA: ${upstream_sha}
      - Origin SHA before sync: ${origin_sha}
      - Local commits ahead of upstream: ${ahead}
      - Upstream commits ahead of origin: ${behind}
      - Conflict files: ${conflict_files:-none}
      EOF

safe-outputs:
  jobs:
    sync-master:
      description: "Update master directly without a merge commit by either fast-forwarding or rebasing onto upstream/master."
      github-token: ${{ secrets.SYNC_PUSH_TOKEN }}
      permissions:
        contents: write
      inputs:
        strategy:
          type: string
          required: true
          description: "Either fast-forward or rebase."
      steps:
        - uses: actions/checkout@v6
          with:
            fetch-depth: 0
        - name: Configure git identity
          run: |
            git config user.name "github-actions[bot]"
            git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
        - name: Sync master
          env:
            SYNC_STRATEGY: ${{ inputs.strategy }}
          run: |
            set -euo pipefail

            if ! git remote get-url upstream >/dev/null 2>&1; then
              git remote add upstream https://github.com/nix-community/lanzaboote.git
            fi

            git fetch --no-tags origin master
            git fetch --no-tags upstream master

            if [ "${SYNC_STRATEGY}" = "fast-forward" ]; then
              git push origin upstream/master:refs/heads/master
            elif [ "${SYNC_STRATEGY}" = "rebase" ]; then
              git checkout -B upstream-sync-write origin/master
              git rebase upstream/master
              git push --force-with-lease origin HEAD:refs/heads/master
            else
              echo "Unsupported strategy: ${SYNC_STRATEGY}" >&2
              exit 1
            fi
    open-sync-pr:
      description: "Create or update a pull request when upstream sync cannot be applied directly."
      github-token: ${{ secrets.SYNC_PUSH_TOKEN }}
      permissions:
        contents: write
        pull-requests: write
      inputs:
        title:
          type: string
          required: true
          description: "Pull request title."
        body:
          type: string
          required: true
          description: "Pull request body."
      steps:
        - uses: actions/checkout@v6
          with:
            fetch-depth: 0
        - name: Configure git identity
          run: |
            git config user.name "github-actions[bot]"
            git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
        - name: Push sync branch and create or update PR
          env:
            GH_TOKEN: ${{ github.token }}
            PR_TITLE: ${{ inputs.title }}
            PR_BODY: ${{ inputs.body }}
          run: |
            set -euo pipefail

            sync_branch="sync/upstream-master"
            base_branch="master"

            if ! git remote get-url upstream >/dev/null 2>&1; then
              git remote add upstream https://github.com/nix-community/lanzaboote.git
            fi

            git fetch --no-tags upstream master
            git checkout -B "${sync_branch}" upstream/master
            git push --force origin "HEAD:refs/heads/${sync_branch}"

            existing_pr="$(gh pr list \
              --head "${sync_branch}" \
              --base "${base_branch}" \
              --state open \
              --json number \
              --jq '.[0].number // empty')"

            if [ -n "${existing_pr}" ]; then
              gh pr edit "${existing_pr}" \
                --title "${PR_TITLE}" \
                --body "${PR_BODY}"
            else
              gh pr create \
                --base "${base_branch}" \
                --head "${sync_branch}" \
                --title "${PR_TITLE}" \
                --body "${PR_BODY}"
            fi
---

# sync-upstream

Read `/tmp/gh-aw/upstream-sync.env` and `/tmp/gh-aw/upstream-sync-report.md`.

This workflow keeps this repository aligned with `nix-community/lanzaboote:master` and must never use merge commits.

## Task

- If `SYNC_STATUS` is `noop`, do nothing.
- If `SYNC_STATUS` is `fast-forward`, call the `sync-master` tool once with `strategy: fast-forward`.
- If `SYNC_STATUS` is `rebase`, call the `sync-master` tool once with `strategy: rebase`.
- If `SYNC_STATUS` is `conflict`, call the `open-sync-pr` tool once.

## Pull Request Requirements

- Use the title `Sync upstream master from nix-community/lanzaboote`.
- State that the daily upstream sync could not update `master` directly because rebasing local-only commits onto `nix-community/lanzaboote:master` produced conflicts.
- Include the ahead and behind counts from the report.
- If conflict files are listed, include them in a short bullet list.
- Add a short manual resolution checklist.
- Keep the body concise and factual.

## Constraints

- Treat `/tmp/gh-aw/upstream-sync.env` as the source of truth.
- Invoke at most one safe output job.
- Never request or create a merge commit.
