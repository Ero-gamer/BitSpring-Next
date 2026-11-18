#!/bin/sh
# Vercel "Ignored Build Step" (vercel.json ignoreCommand).
# Exit 0 = skip this deployment, exit 1 = build it.
#
# The site is built only from these paths, so a commit that touches none of
# them (engine, desktop app, CI, README...) leaves bitspring.app as it is.
SITE_PATHS="apps/website vercel.json .vercelignore"

build() { echo "Building: $1"; exit 1; }
skip() { echo "Skipping: $1"; exit 0; }

git rev-parse --git-dir >/dev/null 2>&1 || build "no git history in the build folder"

# Compare with the branch's last successful deployment, so a push of several
# commits is judged as a whole, not only by its last commit.
BASE="$VERCEL_GIT_PREVIOUS_SHA"
if [ -z "$BASE" ]; then
  # A branch's first deployment: compare with where it left main.
  [ "$VERCEL_GIT_COMMIT_REF" = "main" ] && build "first deployment of main"
  git fetch --quiet --depth=500 origin main 2>/dev/null || build "could not fetch main"
  BASE=$(git merge-base HEAD FETCH_HEAD 2>/dev/null) || build "branch has no common commit with main"
fi

# Vercel clones shallow; reach further back when the base is older than that.
if ! git cat-file -e "$BASE^{commit}" 2>/dev/null; then
  git fetch --quiet --deepen=500 origin 2>/dev/null
  git cat-file -e "$BASE^{commit}" 2>/dev/null || build "base commit $BASE is not in the clone"
fi

if git diff --quiet "$BASE" HEAD -- $SITE_PATHS; then
  skip "the site is unchanged since $BASE"
fi
build "the site changed since $BASE"
