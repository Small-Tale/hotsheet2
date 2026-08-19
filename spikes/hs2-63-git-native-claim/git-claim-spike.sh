#!/usr/bin/env bash
# Git-native claim marker spike (HS2-63) — local bare-repo proof.
# Proves: (1) create-on-push = CAS (2nd claimant rejected), (2) --force-with-lease
# renew/steal is safe under concurrency, (3) cleanup, for a custom ref AND a tag.
set -u
export GIT_AUTHOR_NAME=spike GIT_AUTHOR_EMAIL=spike@test
export GIT_COMMITTER_NAME=spike GIT_COMMITTER_EMAIL=spike@test
EMPTY_TREE=4b825dc642cb6eb9a060e54bf8d69288fbee4904   # universal empty-tree oid

ROOT="${1:?pass a writable ROOT dir}"; rm -rf "$ROOT"; mkdir -p "$ROOT"; cd "$ROOT"
git init -q --bare remote.git
git clone -q --template= remote.git A
git clone -q --template= remote.git B
# seed the remote with a main branch so it's a normal repo
( cd A && git commit -q --allow-empty -m init && git push -q origin HEAD:refs/heads/main )
( cd B && git fetch -q )

payload() { # $1=repo $2=json -> prints new orphan commit oid carrying the payload
  ( cd "$1" && echo "$2" | git commit-tree "$EMPTY_TREE" ); }

pass() { echo "  ✅ $*"; }
fail() { echo "  ❌ $*"; }

echo "== TEST 1: custom ref  refs/hotsheet/claims/T1  — CAS on create =="
CA=$(payload A '{"worker":"A","expires_at":"t+30m"}')
CB=$(payload B '{"worker":"B","expires_at":"t+30m"}')
( cd A && git update-ref refs/hotsheet/claims/T1 "$CA" && git push -q origin refs/hotsheet/claims/T1 ) \
  && pass "worker A claimed (push accepted)" || fail "A push failed (remote rejects custom refs?)"
( cd B && git update-ref refs/hotsheet/claims/T1 "$CB" && git push origin refs/hotsheet/claims/T1 ) 2>b.err
if [ $? -ne 0 ]; then pass "worker B REJECTED (CAS held)"; grep -iE 'reject|fast-forward|already|denied' b.err | sed 's/^/       /'; else fail "worker B push SUCCEEDED — no CAS!"; fi

echo "== TEST 2: tag  refs/tags/hs-claim/T2  — CAS on create =="
TA=$(payload A '{"worker":"A"}'); TB=$(payload B '{"worker":"B"}')
( cd A && git tag -f hs-claim/T2 "$TA" >/dev/null 2>&1; git push -q origin refs/tags/hs-claim/T2 ) \
  && pass "worker A claimed via tag" || fail "A tag push failed"
( cd B && git tag hs-claim/T2 "$TB" >/dev/null 2>&1; git push origin refs/tags/hs-claim/T2 ) 2>bt.err
if [ $? -ne 0 ]; then pass "worker B tag REJECTED (CAS held)"; grep -iE 'reject|already|exists|fast-forward|denied' bt.err | sed 's/^/       /'; else fail "worker B tag push SUCCEEDED — no CAS!"; fi

echo "== TEST 3: renew via --force-with-lease (owner) =="
A2=$(payload A '{"worker":"A","expires_at":"t+60m","renewed":true}')
( cd A && git update-ref refs/hotsheet/claims/T1 "$A2" \
    && git push -q --force-with-lease="refs/hotsheet/claims/T1:$CA" origin refs/hotsheet/claims/T1 ) \
  && pass "owner A renewed (force-with-lease matched old $CA)" || fail "renew failed"

echo "== TEST 4: expired-claim STEAL is mutually exclusive (two stealers, one wins) =="
( cd B && git fetch -q origin '+refs/hotsheet/claims/*:refs/hotsheet/claims/*' )
CUR=$( cd B && git rev-parse refs/hotsheet/claims/T1 )   # both stealers see current = A2
B2=$(payload B '{"worker":"B","stole":true}')
( cd B && git update-ref refs/hotsheet/claims/T1 "$B2" \
    && git push -q --force-with-lease="refs/hotsheet/claims/T1:$CUR" origin refs/hotsheet/claims/T1 ) \
  && pass "stealer B won (force-with-lease matched current $CUR)" || fail "B steal failed"
# A now tries to steal with the STALE value it last knew (A2 == $CUR) -> must fail
A3=$(payload A '{"worker":"A","stole":true}')
( cd A && git update-ref refs/hotsheet/claims/T1 "$A3" \
    && git push --force-with-lease="refs/hotsheet/claims/T1:$CUR" origin refs/hotsheet/claims/T1 ) 2>a.err
if [ $? -ne 0 ]; then pass "concurrent stealer A REJECTED (stale lease)"; grep -iE 'reject|stale|fast-forward' a.err | sed 's/^/       /'; else fail "both stealers won — NOT mutually exclusive!"; fi

echo "== TEST 5: enumerate + cleanup =="
echo "  markers on remote:"; git ls-remote remote.git 'refs/hotsheet/claims/*' 'refs/tags/hs-claim/*' | sed 's/^/       /'
( cd A && git push -q origin :refs/hotsheet/claims/T1 :refs/tags/hs-claim/T2 ) \
  && pass "swept (deleted ref + tag)" || fail "cleanup failed"
echo "  after sweep:"; git ls-remote remote.git 'refs/hotsheet/claims/*' 'refs/tags/hs-claim/*' | sed 's/^/       /'; echo "       (empty = good)"

cd /; rm -rf "$ROOT"
echo "== done =="
