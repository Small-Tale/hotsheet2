#!/usr/bin/env bash
# HS2-63 GitHub probe: does GitHub accept a custom ref namespace vs a tag?
# Pushes ONE test custom-ref + ONE test tag pointing at the existing HEAD commit
# (no new objects), observes accept/reject, then DELETES both. Idempotent cleanup.
set -u
cd "$(git rev-parse --show-toplevel)" || { echo "run inside the repo"; exit 1; }
OID=$(git rev-parse HEAD)
CUSTOM=refs/hotsheet/claims/spike-probe
TAG=refs/tags/hs-claim-spike-probe

cleanup() {
  echo "== cleanup (delete probes) =="
  git push origin ":$CUSTOM" 2>&1 | sed 's/^/   /' || true
  git push origin ":$TAG"    2>&1 | sed 's/^/   /' || true
  echo "   remaining probe refs on origin:"
  git ls-remote origin "$CUSTOM" "$TAG" | sed 's/^/     /'
  echo "     (empty = clean)"
}
trap cleanup EXIT

echo "== PROBE 1: custom ref  $CUSTOM =="
if git push origin "$OID:$CUSTOM" 2>&1 | sed 's/^/   /'; then
  echo "   RESULT: custom ref ACCEPTED by GitHub"
else
  echo "   RESULT: custom ref REJECTED by GitHub"
fi
echo
echo "== PROBE 2: tag  $TAG =="
if git push origin "$OID:$TAG" 2>&1 | sed 's/^/   /'; then
  echo "   RESULT: tag ACCEPTED by GitHub"
else
  echo "   RESULT: tag REJECTED by GitHub"
fi
echo
echo "== what is visible on origin now =="
git ls-remote origin "refs/hotsheet/*" "$TAG" | sed 's/^/   /'; echo "   (end)"
