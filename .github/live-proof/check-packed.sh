#!/usr/bin/env bash
set -euo pipefail
QUAL="${FRILADD_QUALIFICATION_ROOT:?}"
TARBALL=$1
RUN=$(mktemp -d "$QUAL/packed-run.XXXXXX")
printf 'Isolated run %s\n' "$RUN"
export FRILADD_CONFIG_DIR="$RUN/friladd-global"
export NPM_CONFIG_USERCONFIG="$QUAL/user.npmrc" NPM_CONFIG_GLOBALCONFIG="$QUAL/global.npmrc" NPM_CONFIG_CACHE="$QUAL/npm-cache"
export NPM_CONFIG_REGISTRY=https://registry.npmjs.org
mkdir "$RUN/copy" "$RUN/managed" "$RUN/legacy"
shasum -a 256 "$TARBALL"
tar -tzf "$TARBALL" | sort
npm install -g --ignore-scripts --no-audit --no-fund --prefix "$RUN/bin-prefix" "$TARBALL"
export PATH="$RUN/bin-prefix/bin:$PATH"
fril() {
  local attempt result
  for attempt in 1 2 3; do
    if command fril "$@" >"$RUN/command.stdout" 2>"$RUN/command.stderr"; then
      result=0
    else
      result=$?
    fi
    if [ "$attempt" -lt 3 ] && grep -Eq 'Public registry request failed|Cannot resolve approved public registry host' "$RUN/command.stderr"; then
      printf 'Network retry %s for fril %s\n' "$attempt" "$*" >&2
      cat "$RUN/command.stderr" >&2
    else
      cat "$RUN/command.stdout"
      cat "$RUN/command.stderr" >&2
      return "$result"
    fi
  done
}
test "$(fril --version)" = 'fril 0.2.1'
test "$(friladd --version)" = 'friladd 0.2.1'
printf 'Both aliases report 0.2.1\n'

REV=a5e57a4cda0a12c4dcce906ea98d20a94f1346a5
DIGEST=963bbf8bdb7078921bc24bec1523e90d77a50e1b6f4489e44c841be0f581cc22
MANIFEST=https://raw.githubusercontent.com/FrilLab/frilcraft/$REV/registry.json
curl --fail --silent --show-error --proto '=https' --max-time 30 -D "$RUN/headers" -o "$RUN/catalog.json" https://registry.frillab.com/registries.json
curl --fail --silent --show-error --proto '=https' --max-time 30 -o "$RUN/manifest.json" "$MANIFEST"
jq -e --arg rev "$REV" --arg digest "$DIGEST" '.registries[] | select(.id == "frilcraft" and .status == "active" and .reviewedRevision == $rev and .manifestSha256 == $digest)' "$RUN/catalog.json" >/dev/null
printf '%s  %s\n' "$DIGEST" "$RUN/manifest.json" | shasum -a 256 -c
jq -r '.items[] | select(.name == "envelope-reveal") | .files[] | [.sha256,.target] | join("  ")' "$RUN/manifest.json" > "$RUN/resource.sha256"

cd "$RUN/copy"
fril registry discover
fril list --public
fril search envelope
fril info frilcraft/envelope-reveal
fril add frilcraft/envelope-reveal --dry-run
test -z "$(find . -mindepth 1 -print -quit)"
test ! -e "$FRILADD_CONFIG_DIR"
fril add frilcraft/envelope-reveal
shasum -a 256 -c "$RUN/resource.sha256"
find . -mindepth 1 | LC_ALL=C sort > "$RUN/copy-tree.txt"
diff -u - "$RUN/copy-tree.txt" <<'TREE'
./src
./src/components
./src/components/envelope-reveal
./src/components/envelope-reveal/EnvelopeReveal.tsx
./src/components/envelope-reveal/LICENSE
./src/components/envelope-reveal/index.ts
TREE
grep -Eq '^MIT License' src/components/envelope-reveal/LICENSE
fril installed --json | jq -e '.data.items == []' >/dev/null
set +e
fril add frilcraft/envelope-reveal --json > "$RUN/repeat.stdout" 2> "$RUN/repeat.stderr"
REPEAT=$?
set -e
test "$REPEAT" -eq 2
test ! -s "$RUN/repeat.stdout"
grep -Eq 'already exist and are unmanaged' "$RUN/repeat.stderr"
shasum -a 256 -c "$RUN/resource.sha256"
printf 'Copy file tree and repeated no-clobber add passed\n'

mkdir "$RUN/seeded"
cd "$RUN/seeded"
printf '%s\n' '{"name":"existing-project","version":"1.0.0"}' > package.json
printf '%s\n' '{"name":"existing-project","lockfileVersion":3}' > package-lock.json
printf '%s\n' 'node_modules/' > .gitignore
shasum -a 256 package.json package-lock.json .gitignore > "$RUN/seeded.sha256"
fril add frilcraft/envelope-reveal
shasum -a 256 -c "$RUN/seeded.sha256"
test ! -e .friladd
test ! -e .friladd.json
test ! -e friladd.lock.json

cd "$RUN/managed"
fril add frilcraft/envelope-reveal --managed --dry-run
test -z "$(find . -mindepth 1 -print -quit)"
fril add frilcraft/envelope-reveal --managed
fril installed
fril status --check
jq -e --arg digest "$DIGEST" '.schemaVersion == 1 and .items[0].manifestSha256 == $digest and (.items[0].files | length == 3)' friladd.lock.json >/dev/null
cp friladd.lock.json "$RUN/managed-lock-before.json"
fril add frilcraft/envelope-reveal
cmp friladd.lock.json "$RUN/managed-lock-before.json"
fril update envelope-reveal
cmp friladd.lock.json "$RUN/managed-lock-before.json"
rm src/components/envelope-reveal/index.ts
fril restore envelope-reveal
shasum -a 256 -c "$RUN/resource.sha256"
cmp friladd.lock.json "$RUN/managed-lock-before.json"
printf 'consumer edit\n' > src/components/envelope-reveal/index.ts
set +e
fril update envelope-reveal --json > "$RUN/edited.stdout" 2> "$RUN/edited.stderr"
EDITED=$?
set -e
test "$EDITED" -eq 2
test ! -s "$RUN/edited.stdout"
grep -Eq 'modified' "$RUN/edited.stderr"
test "$(cat src/components/envelope-reveal/index.ts)" = 'consumer edit'
cmp friladd.lock.json "$RUN/managed-lock-before.json"
rm src/components/envelope-reveal/index.ts
fril restore envelope-reveal
printf 'untracked\n' > src/components/envelope-reveal/keep
fril remove envelope-reveal
test "$(find src -type f)" = 'src/components/envelope-reveal/keep'
test "$(cat src/components/envelope-reveal/keep)" = 'untracked'
test -f .friladd/mutation.lock
fril installed --json | jq -e '.data.items == []' >/dev/null
test ! -e "$FRILADD_CONFIG_DIR"
printf 'Managed status, same-snapshot update, edit protection, exact restore and owned removal passed\n'

cd "$RUN/legacy"
LEGACY="$QUAL/legacy-release/friladd-x86_64-unknown-linux-gnu"
"$LEGACY" registry add frilcraft "$MANIFEST" --default
"$LEGACY" add envelope-reveal
cp .friladd.json "$RUN/old-config.json"
cp friladd.lock.json "$RUN/old-lock.json"
fril installed
fril status --check
fril add frilcraft/envelope-reveal
rm src/components/envelope-reveal/EnvelopeReveal.tsx
fril restore frilcraft/envelope-reveal
shasum -a 256 -c "$RUN/resource.sha256"
cmp .friladd.json "$RUN/old-config.json"
cmp friladd.lock.json "$RUN/old-lock.json"
fril update frilcraft/envelope-reveal --registry "$MANIFEST"
cmp friladd.lock.json "$RUN/old-lock.json"
fril remove frilcraft/envelope-reveal
test -z "$(find src -type f -print -quit)"
test ! -e "$FRILADD_CONFIG_DIR"
printf 'Checksum-verified v0.2.0 installation survived upgrade, including ownership and config bytes\n'

cd "$RUN/copy"
cp "$QUAL"/consumer-template/{package.json,package-lock.json,tsconfig.json,vite.config.ts,index.html} .
cp "$QUAL"/consumer-template/{main.tsx,consumer.css} src/
npm ci --ignore-scripts --no-audit --no-fund
npm run typecheck
npm run build
node "$QUAL/browser-tool/check.mjs" "$RUN/copy/dist"
shasum -a 256 -c "$RUN/resource.sha256"
printf 'Production integration passed for %s\n' "$TARBALL"
