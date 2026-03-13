---
name: Sync With Upstream

on:
  workflow_dispatch:
  schedule: weekly on monday

permissions:
  contents: read
  pull-requests: read

engine:
  id: claude
  env:
    ANTHROPIC_BASE_URL: https://api.z.ai/api/anthropic
    ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
    API_TIMEOUT_MS: "3000000"

network:
  allowed:
    - defaults
    - api.z.ai

safe-outputs:
  jobs:
    sync-branch:
      description: Force-push a fast-forwarded or cleanly rebased target branch without creating merge commits.
      inputs:
        expected_origin_sha:
          type: string
          description: Current SHA of the target branch observed during analysis.
          required: true
        expected_upstream_sha:
          type: string
          description: Current SHA of the upstream branch observed during analysis.
          required: true
        target_branch:
          type: string
          description: Branch in this repository to update.
          required: true
        upstream_repo:
          type: string
          description: Upstream repository in owner/name form.
          required: true
        upstream_branch:
          type: string
          description: Upstream branch name to sync from.
          required: true
      permissions:
        contents: write
      steps:
        - uses: actions/checkout@v6
          with:
            fetch-depth: 0
            ref: ${{ inputs.target_branch }}
        - name: Synchronize branch
          shell: bash
          env:
            EXPECTED_ORIGIN_SHA: ${{ inputs.expected_origin_sha }}
            EXPECTED_UPSTREAM_SHA: ${{ inputs.expected_upstream_sha }}
            TARGET_BRANCH: ${{ inputs.target_branch }}
            UPSTREAM_REPO: ${{ inputs.upstream_repo }}
            UPSTREAM_BRANCH: ${{ inputs.upstream_branch }}
          run: |
            set -euo pipefail

            git config user.name "github-actions[bot]"
            git config user.email "41898282+github-actions[bot]@users.noreply.github.com"

            if git remote get-url upstream >/dev/null 2>&1; then
              git remote set-url upstream "https://github.com/${UPSTREAM_REPO}.git"
            else
              git remote add upstream "https://github.com/${UPSTREAM_REPO}.git"
            fi

            git fetch --no-tags --prune origin "${TARGET_BRANCH}"
            git fetch --no-tags --prune upstream "${UPSTREAM_BRANCH}"

            origin_sha="$(git rev-parse "origin/${TARGET_BRANCH}")"
            upstream_sha="$(git rev-parse "upstream/${UPSTREAM_BRANCH}")"

            if [[ "${origin_sha}" != "${EXPECTED_ORIGIN_SHA}" ]]; then
              echo "origin/${TARGET_BRANCH} moved from ${EXPECTED_ORIGIN_SHA} to ${origin_sha}; rerun the workflow."
              exit 1
            fi

            if [[ "${upstream_sha}" != "${EXPECTED_UPSTREAM_SHA}" ]]; then
              echo "upstream/${UPSTREAM_BRANCH} moved from ${EXPECTED_UPSTREAM_SHA} to ${upstream_sha}; rerun the workflow."
              exit 1
            fi

            git checkout -B sync-work "origin/${TARGET_BRANCH}"

            if git merge-base --is-ancestor "origin/${TARGET_BRANCH}" "upstream/${UPSTREAM_BRANCH}"; then
              git reset --hard "upstream/${UPSTREAM_BRANCH}"
            else
              if ! git rebase "upstream/${UPSTREAM_BRANCH}"; then
                git rebase --abort || true
                echo "Automatic rebase failed; open a PR instead."
                exit 1
              fi
            fi

            git push origin "HEAD:refs/heads/${TARGET_BRANCH}" \
              --force-with-lease="refs/heads/${TARGET_BRANCH}:${EXPECTED_ORIGIN_SHA}"

    open-sync-pr:
      description: Push an upstream sync branch and create or reuse a pull request when automatic rebase is blocked by conflicts.
      inputs:
        expected_origin_sha:
          type: string
          description: Current SHA of the target branch observed during analysis.
          required: true
        expected_upstream_sha:
          type: string
          description: Current SHA of the upstream branch observed during analysis.
          required: true
        target_branch:
          type: string
          description: Branch in this repository that should receive the sync.
          required: true
        upstream_repo:
          type: string
          description: Upstream repository in owner/name form.
          required: true
        upstream_branch:
          type: string
          description: Upstream branch name to sync from.
          required: true
        conflict_summary:
          type: string
          description: Short explanation of why the PR is needed.
          required: true
      permissions:
        contents: write
        pull-requests: write
      steps:
        - uses: actions/checkout@v6
          with:
            fetch-depth: 0
            ref: ${{ inputs.target_branch }}
        - name: Push sync branch and open PR
          shell: bash
          env:
            GH_TOKEN: ${{ github.token }}
            EXPECTED_ORIGIN_SHA: ${{ inputs.expected_origin_sha }}
            EXPECTED_UPSTREAM_SHA: ${{ inputs.expected_upstream_sha }}
            TARGET_BRANCH: ${{ inputs.target_branch }}
            UPSTREAM_REPO: ${{ inputs.upstream_repo }}
            UPSTREAM_BRANCH: ${{ inputs.upstream_branch }}
            CONFLICT_SUMMARY: ${{ inputs.conflict_summary }}
          run: |
            set -euo pipefail

            if git remote get-url upstream >/dev/null 2>&1; then
              git remote set-url upstream "https://github.com/${UPSTREAM_REPO}.git"
            else
              git remote add upstream "https://github.com/${UPSTREAM_REPO}.git"
            fi

            git fetch --no-tags --prune origin "${TARGET_BRANCH}"
            git fetch --no-tags --prune upstream "${UPSTREAM_BRANCH}"

            origin_sha="$(git rev-parse "origin/${TARGET_BRANCH}")"
            upstream_sha="$(git rev-parse "upstream/${UPSTREAM_BRANCH}")"

            if [[ "${origin_sha}" != "${EXPECTED_ORIGIN_SHA}" ]]; then
              echo "origin/${TARGET_BRANCH} moved from ${EXPECTED_ORIGIN_SHA} to ${origin_sha}; rerun the workflow."
              exit 1
            fi

            if [[ "${upstream_sha}" != "${EXPECTED_UPSTREAM_SHA}" ]]; then
              echo "upstream/${UPSTREAM_BRANCH} moved from ${EXPECTED_UPSTREAM_SHA} to ${upstream_sha}; rerun the workflow."
              exit 1
            fi

            short_sha="${upstream_sha:0:12}"
            sync_branch="sync/upstream-${UPSTREAM_BRANCH}-${short_sha}"

            git checkout --detach "${upstream_sha}"
            git push origin "HEAD:refs/heads/${sync_branch}" --force

            title="Sync ${UPSTREAM_REPO}:${UPSTREAM_BRANCH} into ${TARGET_BRANCH}"
            body_file="$(mktemp)"
            cat >"${body_file}" <<EOF
            This automated sync PR updates \`${TARGET_BRANCH}\` to upstream commit \`${upstream_sha}\` from \`${UPSTREAM_REPO}:${UPSTREAM_BRANCH}\`.

            Automatic sync could not be applied directly because a clean rebase was not possible without conflicts.

            Conflict summary: ${CONFLICT_SUMMARY}

            This PR uses the upstream tip as-is so conflicts can be resolved in review without creating merge commits.
            EOF

            existing_pr="$(gh pr list \
              --base "${TARGET_BRANCH}" \
              --head "${sync_branch}" \
              --state open \
              --json number \
              --jq '.[0].number // empty')"

            if [[ -n "${existing_pr}" ]]; then
              gh pr edit "${existing_pr}" --title "${title}" --body-file "${body_file}"
            else
              gh pr create \
                --base "${TARGET_BRANCH}" \
                --head "${sync_branch}" \
                --title "${title}" \
                --body-file "${body_file}"
            fi

steps:
  - name: Analyze upstream sync feasibility
    shell: bash
    env:
      TARGET_BRANCH: master
      UPSTREAM_REPO: nix-community/lanzaboote
      UPSTREAM_BRANCH: master
      REPORT_PATH: .github/workflows/.parent-sync-report.md
    run: |
      set -euo pipefail

      git fetch --no-tags --prune --unshallow origin "${TARGET_BRANCH}" 2>/dev/null || true
      git fetch --no-tags --prune origin "${TARGET_BRANCH}"

      if git remote get-url upstream >/dev/null 2>&1; then
        git remote set-url upstream "https://github.com/${UPSTREAM_REPO}.git"
      else
        git remote add upstream "https://github.com/${UPSTREAM_REPO}.git"
      fi

      git fetch --no-tags --prune upstream "${UPSTREAM_BRANCH}"

      origin_sha="$(git rev-parse "origin/${TARGET_BRANCH}")"
      upstream_sha="$(git rev-parse "upstream/${UPSTREAM_BRANCH}")"
      merge_base="$(git merge-base "origin/${TARGET_BRANCH}" "upstream/${UPSTREAM_BRANCH}")"

      status=""
      recommended_action=""
      summary=""

      if [[ "${origin_sha}" == "${upstream_sha}" ]]; then
        status="noop"
        recommended_action="none"
        summary="The target branch already matches upstream."
      elif git merge-base --is-ancestor "origin/${TARGET_BRANCH}" "upstream/${UPSTREAM_BRANCH}"; then
        status="fast-forward"
        recommended_action="sync-branch"
        summary="The target branch can be fast-forwarded to the upstream tip."
      else
        worktree_dir="$(mktemp -d)"
        cleanup() {
          git worktree remove --force "${worktree_dir}" >/dev/null 2>&1 || true
        }
        trap cleanup EXIT

        git worktree add --detach "${worktree_dir}" "origin/${TARGET_BRANCH}" >/dev/null
        if (
          cd "${worktree_dir}"
          git rebase "upstream/${UPSTREAM_BRANCH}" >/tmp/parent-sync-rebase.log 2>&1
        ); then
          status="clean-rebase"
          recommended_action="sync-branch"
          summary="The target branch has local commits but rebases cleanly onto upstream."
        else
          status="conflict"
          recommended_action="open-sync-pr"
          summary="Rebasing the target branch onto upstream produced conflicts."
        fi

        cleanup
        trap - EXIT
      fi

      cat >"${REPORT_PATH}" <<EOF
      # Parent Sync Report

      - Target branch: ${TARGET_BRANCH}
      - Upstream: ${UPSTREAM_REPO}:${UPSTREAM_BRANCH}
      - Current target SHA: ${origin_sha}
      - Current upstream SHA: ${upstream_sha}
      - Merge base: ${merge_base}
      - Status: ${status}
      - Recommended action: ${recommended_action}
      - Summary: ${summary}

      ## Rules

      - Never create a merge commit.
      - If the recommended action is \`sync-branch\`, update \`${TARGET_BRANCH}\` directly.
      - If the recommended action is \`open-sync-pr\`, open a PR from a sync branch that points at the upstream tip.
      - If the recommended action is \`none\`, do nothing.
      EOF

---

# Weekly parent sync

Read `.github/workflows/.parent-sync-report.md` and follow its recommendation exactly.

## Required behavior

1. If the report says `Recommended action: none`, make no safe-output calls and finish with a short explanation.
2. If the report says `Recommended action: sync-branch`, call `sync-branch` exactly once with:
   - `expected_origin_sha`: the report's `Current target SHA`
   - `expected_upstream_sha`: the report's `Current upstream SHA`
   - `target_branch`: `master`
   - `upstream_repo`: `nix-community/lanzaboote`
   - `upstream_branch`: `master`
3. If the report says `Recommended action: open-sync-pr`, call `open-sync-pr` exactly once with:
   - `expected_origin_sha`: the report's `Current target SHA`
   - `expected_upstream_sha`: the report's `Current upstream SHA`
   - `target_branch`: `master`
   - `upstream_repo`: `nix-community/lanzaboote`
   - `upstream_branch`: `master`
   - `conflict_summary`: a single-sentence summary based on the report

## Constraints

- Never call both safe-output jobs.
- Never create merge commits.
- Do not change any branch other than `master` or the sync branch created by `open-sync-pr`.
- Do not use a merge-based PR branch. The sync PR branch must point at the upstream tip.
