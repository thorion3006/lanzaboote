---
on:
  schedule: weekly
  workflow_dispatch:

description: "Weekly sync from nix-community/lanzaboote:master without merge commits"

permissions:
  contents: read
  pull-requests: read

engine:
  id: claude
  env:
    ANTHROPIC_BASE_URL: "https://api.z.ai/api/anthropic"

network:
  allowed:
    - api.z.ai
    - defaults
    - github

concurrency:
  group: gh-aw-weekly-upstream-sync-${{ github.ref_name }}
  cancel-in-progress: false

safe-outputs:
  jobs:
    upstream-sync:
      description: "Sync master with nix-community/lanzaboote:master without merge commits"
      runs-on: ubuntu-latest
      permissions:
        contents: write
        pull-requests: write
      output: "Upstream sync job completed."
      inputs:
        base_branch:
          description: "Branch to keep in sync"
          required: true
          type: string
        upstream_repo:
          description: "Upstream repository in owner/name format"
          required: true
          type: string
        upstream_branch:
          description: "Upstream branch to sync from"
          required: true
          type: string
        sync_branch:
          description: "Branch name to reuse for conflict PRs"
          required: true
          type: string
        pr_title:
          description: "Title to use when opening a conflict PR"
          required: true
          type: string
        pr_body:
          description: "Body to use when opening a conflict PR"
          required: true
          type: string
      steps:
        - name: Check out repository
          uses: actions/checkout@v4
          with:
            fetch-depth: 0
            persist-credentials: true
            token: ${{ secrets.GH_AW_SYNC_TOKEN }}

        - name: Sync with upstream
          env:
            GH_TOKEN: ${{ secrets.GH_AW_SYNC_TOKEN }}
          run: |
            set -euo pipefail

            payload="$(cat "$GH_AW_AGENT_OUTPUT")"

            base_branch="$(jq -r '.items[] | select(.type == "upstream_sync") | .base_branch' <<<"$payload")"
            upstream_repo="$(jq -r '.items[] | select(.type == "upstream_sync") | .upstream_repo' <<<"$payload")"
            upstream_branch="$(jq -r '.items[] | select(.type == "upstream_sync") | .upstream_branch' <<<"$payload")"
            sync_branch="$(jq -r '.items[] | select(.type == "upstream_sync") | .sync_branch' <<<"$payload")"
            pr_title="$(jq -r '.items[] | select(.type == "upstream_sync") | .pr_title' <<<"$payload")"
            pr_body="$(jq -r '.items[] | select(.type == "upstream_sync") | .pr_body' <<<"$payload")"

            if [ -z "$base_branch" ] || [ -z "$upstream_repo" ] || [ -z "$upstream_branch" ] || [ -z "$sync_branch" ]; then
              echo "Missing required sync parameters"
              exit 1
            fi

            git config user.name "github-actions[bot]"
            git config user.email "41898282+github-actions[bot]@users.noreply.github.com"

            git remote remove upstream 2>/dev/null || true
            git remote add upstream "https://github.com/${upstream_repo}.git"

            git fetch --no-tags origin "${base_branch}"
            git fetch --no-tags upstream "${upstream_branch}"

            origin_ref="refs/remotes/origin/${base_branch}"
            upstream_ref="refs/remotes/upstream/${upstream_branch}"
            origin_sha="$(git rev-parse "${origin_ref}")"
            upstream_sha="$(git rev-parse "${upstream_ref}")"

            echo "origin ${base_branch}: ${origin_sha}"
            echo "upstream ${upstream_branch}: ${upstream_sha}"

            if [ "${origin_sha}" = "${upstream_sha}" ]; then
              echo "Branch is already up to date."
              exit 0
            fi

            work_branch="gh-aw-sync-work"
            git checkout -B "${work_branch}" "${origin_ref}"

            if git merge-base --is-ancestor "${origin_sha}" "${upstream_sha}"; then
              echo "Fast-forwarding ${base_branch} to ${upstream_sha}"
              git push origin "${upstream_sha}:refs/heads/${base_branch}"
              exit 0
            fi

            set +e
            git rebase "${upstream_ref}"
            rebase_status=$?
            set -e

            if [ "${rebase_status}" -eq 0 ]; then
              rebased_sha="$(git rev-parse HEAD)"
              echo "Rebased ${base_branch} to ${rebased_sha}"
              git push --force-with-lease="refs/heads/${base_branch}:${origin_sha}" origin "${rebased_sha}:refs/heads/${base_branch}"
              exit 0
            fi

            echo "Rebase hit conflicts. Trying the held-paths policy before falling back to a PR."
            git rebase --abort || true

            # Only (re)create the sync branch when it does not exist yet or still points at
            # an (older) upstream commit. If it carries a manual conflict resolution, the
            # force-push below would destroy that work. A resolution commit created by the
            # held-paths policy below is regenerable, so a sync branch whose tip is such a
            # policy commit is recreated on top of the newer upstream instead of stalling
            # all later syncs.
            remote_sync_sha="$(git ls-remote origin "refs/heads/${sync_branch}" | cut -f1)"
            if [ -n "${remote_sync_sha}" ] && ! git merge-base --is-ancestor "${remote_sync_sha}" "${upstream_sha}"; then
              git fetch --no-tags origin "${sync_branch}:${sync_branch}" >/dev/null 2>&1 || true
              remote_sync_tip="$(git log -1 --format=%s "${remote_sync_sha}" 2>/dev/null || true)"
              if [ "${remote_sync_tip}" != "Sync with ${upstream_repo}:${upstream_branch} (keep fork side for held paths)" ]; then
                echo "Sync branch ${sync_branch} carries a manual resolution; leaving it and its PR untouched."
                exit 0
              fi
              echo "Sync branch ${sync_branch} carries a policy resolution; recreating it on the newer upstream."
            fi

            # While .github/upstream-sync-keep-ours exists, master deliberately holds the
            # listed paths (adopted from an unmerged upstream PR). If every conflict is
            # confined to those paths, resolve by keeping the fork side there and taking
            # upstream everywhere else, so the sync PR is mergeable as-is. Upstream commits
            # shadowed by keeping our side are listed in the resolution commit message.
            policy_applied=0
            keep_ours_file=".github/upstream-sync-keep-ours"
            if [ -f "${keep_ours_file}" ]; then
              git checkout -B "${work_branch}" "${origin_ref}"
              set +e
              git merge --no-commit --no-ff "${upstream_ref}" >/dev/null 2>&1
              merge_status=$?
              set -e
              conflicted="$(git diff --name-only --diff-filter=U)"
              if [ "${merge_status}" -eq 0 ] || [ -n "${conflicted}" ]; then
                outside_conflicts=""
                while IFS= read -r path; do
                  [ -n "${path}" ] || continue
                  keep=0
                  while IFS= read -r prefix; do
                    case "${prefix}" in ''|'#'*) continue ;; esac
                    case "${path}" in "${prefix}"*) keep=1; break ;; esac
                  done < "${keep_ours_file}"
                  if [ "${keep}" -eq 0 ]; then
                    outside_conflicts="${outside_conflicts}${path}\n"
                  fi
                done <<< "${conflicted}"
                if [ -z "${outside_conflicts}" ]; then
                  while IFS= read -r path; do
                    [ -n "${path}" ] || continue
                    if git rev-parse -q --verify ":2:${path}" >/dev/null 2>&1; then
                      git checkout --ours -- "${path}"
                      git add -- "${path}"
                    else
                      # The fork side deleted this file; keeping the fork side means
                      # keeping the deletion.
                      git rm -f -- "${path}"
                    fi
                  done <<< "${conflicted}"
                  git add -A
                  keep_paths="$(grep -v '^#' "${keep_ours_file}" | grep -v '^$' || true)"
                  shadowed=""
                  if [ -n "${keep_paths}" ]; then
                    shadowed="$(git log --oneline "${origin_sha}..${upstream_sha}" -- ${keep_paths} || true)"
                  fi
                  git commit \
                    -m "Sync with ${upstream_repo}:${upstream_branch} (keep fork side for held paths)" \
                    -m "Rebase conflicts were confined to the paths held in ${keep_ours_file} and were resolved by keeping the fork side there; upstream is taken everywhere else." \
                    -m "Shadowed upstream commits (review these):" \
                    -m "${shadowed:-None}"
                  git checkout -B "${sync_branch}"
                  policy_applied=1
                  echo "Applied held-paths policy; the sync branch carries the resolved sync."
                else
                  echo "Conflicts outside the held paths; manual resolution required:"
                  printf '%b' "${outside_conflicts}"
                  git merge --abort || true
                fi
              else
                echo "Merge attempt failed without conflicts; falling back to a manual PR."
                git merge --abort || true
              fi
            fi

            if [ "${policy_applied}" -eq 0 ]; then
              git checkout -B "${sync_branch}" "${upstream_ref}"
            fi
            git push --force origin "${sync_branch}:refs/heads/${sync_branch}"

            existing_pr="$(gh pr list \
              --base "${base_branch}" \
              --head "${sync_branch}" \
              --state open \
              --json number \
              --jq '.[0].number // empty')"

            if [ -z "${existing_pr}" ]; then
              # The filtered list call can miss same-repo PRs in this sandbox; fall back to
              # a graphql lookup, the same transport the create below uses.
              existing_pr="$(gh api graphql \
                -f query='query($owner:String!,$name:String!,$head:String!){repository(owner:$owner,name:$name){pullRequests(states:OPEN,headRefName:$head,first:1){nodes{number}}}}' \
                -F owner="${GITHUB_REPOSITORY_OWNER}" \
                -F name="${GITHUB_REPOSITORY#*/}" \
                -F head="${sync_branch}" \
                --jq '.data.repository.pullRequests.nodes[0].number // empty')"
            fi

            if [ -n "${existing_pr}" ]; then
              gh pr edit "${existing_pr}" --title "${pr_title}" --body "${pr_body}"
              echo "Updated existing PR #${existing_pr}"
            else
              repo_id="$(gh repo view "${GITHUB_REPOSITORY}" --json id --jq .id)"
              gh api graphql \
                -f query='mutation($repo:ID!,$base:String!,$head:String!,$title:String!,$body:String!){createPullRequest(input:{repositoryId:$repo,baseRefName:$base,headRefName:$head,title:$title,body:$body}){pullRequest{url number}}}' \
                -F repo="${repo_id}" \
                -F base="${base_branch}" \
                -F head="${sync_branch}" \
                -F title="${pr_title}" \
                -F body="${pr_body}"
            fi
---

# Weekly Upstream Sync

Keep this repository's `master` branch aligned with `nix-community/lanzaboote:master` on a weekly cadence.

## Goal

Update the default branch without merge commits.

- If this repository is simply behind upstream, fast-forward `master`.
- If this repository has local commits that can be replayed cleanly, rebase them onto upstream and update `master` with a linear history.
- If the rebase would conflict, check the held-paths policy below: conflicts confined to the held paths are resolved automatically by keeping the fork side; anything else still opens a conflict PR for manual resolution.

## Held paths (temporary, until upstream PR #673 lands)

`master` carries an adopted implementation of [nix-community/lanzaboote#673](https://github.com/nix-community/lanzaboote/pull/673)
in the paths listed in `.github/upstream-sync-keep-ours`. Upstream does not have #673
yet, so rebases conflict in those files. The policy, applied automatically by the tool:

- If every conflicted path is under the held list, the sync is resolved by keeping the
  fork side for exactly those paths and taking upstream everywhere else. The resulting
  sync PR is mergeable as-is, and the resolution commit message lists the shadowed
  upstream commits for review.
- If any conflict falls outside the held list, the sync branch is set to plain upstream
  and the PR requires manual resolution, as before.

Once #673 (ideally with our complementary patches) is merged upstream, remove
`.github/upstream-sync-keep-ours` so syncs revert to plain rebases.

## Token

Use the `GH_AW_SYNC_TOKEN` repository secret for pushing branches and creating pull requests.

## Instructions

1. Fetch `origin/master` and `nix-community/lanzaboote:master`, then inspect the relationship between the two histories with `git merge-base`, `git log`, and similar read-only commands.
2. Never create merge commits on the direct fast-forward/rebase paths. A sync resolved via the held-paths policy intentionally carries one resolution commit on the sync branch, merged through a PR.
3. If the branch is already current, explain that no action is required and stop.
4. Otherwise call the `upstream-sync` tool once with:
   - `base_branch`: `master`
   - `upstream_repo`: `nix-community/lanzaboote`
   - `upstream_branch`: `master`
   - `sync_branch`: `gh-aw/upstream-sync-master`
   - `pr_title`: `Sync master with nix-community/lanzaboote:master`
   - `pr_body`: a short explanation of what happened: if the held-paths policy resolved the sync, say the PR is mergeable, that the fork side was kept for the paths in `.github/upstream-sync-keep-ours`, and that shadowed upstream commits are listed in the resolution commit; otherwise, that the weekly sync hit rebase conflicts outside the held paths and the PR needs manual conflict resolution.
5. Keep the final response concise and factual. State whether the repository was already up to date, was synced directly, or required a conflict PR.
