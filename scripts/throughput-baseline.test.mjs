import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  assignmentTimeline,
  distribution,
  evidenceTicket,
  measure,
  mapMainCommits,
  parseTicket,
  requiredRun,
  runEvidence,
  summarize,
  workerIdleGaps,
} from './throughput-baseline.mjs';

const at = (n) => Date.parse('2026-10-27T10:00:00Z') + n * 60_000;
const iso = (n) => new Date(at(n)).toISOString();
const source = 'a'.repeat(40),
  other = 'b'.repeat(40),
  head = 'c'.repeat(40);

test('HS2-34PWSK registers baseline regressions in local and CI gates', () => {
  for (const relative of ['./check-full.sh', '../.github/workflows/ci.yml']) {
    const source = readFileSync(new URL(relative, import.meta.url), 'utf8');
    const commands = source.split('\n').filter((line) => /\bnode --test\b/.test(line));
    assert.ok(
      commands.some((line) => line.includes('scripts/throughput-baseline.test.mjs')),
      `${relative} must execute the baseline regressions`,
    );
  }
});
const assignment = {
  id: 'op-1',
  slug: 'HS2-SYN001',
  worker: 'w1',
  generation: 'g1',
  phase: 'done',
  commit: source,
  handoff: { kind: 'code', commit: source },
};
const ticket = () => ({
  slug: 'HS2-SYN001',
  status: 'completed',
  claims: [
    { kind: 'claim', worker: 'w1', at: at(2) },
    { kind: 'release', worker: 'w1', at: at(20) },
    { kind: 'claim', worker: 'correction', at: at(25) },
  ],
  notes: [
    { id: 'n1', at: at(1), kind: 'assignment', operations: ['op-1'], commits: [], actorId: 'root' },
    { id: 'n2', at: at(19), kind: 'code', operations: [], commits: [source], actorId: 'w1' },
  ],
});
const run = (id, start, end, extra = {}) => ({
  databaseId: id,
  headSha: head,
  headBranch: 'main',
  event: 'push',
  workflowName: 'CI',
  status: 'completed',
  conclusion: 'success',
  createdAt: iso(start),
  updatedAt: iso(end),
  ...extra,
});
const args = (extra = {}) => ({
  assignment,
  ticket: ticket(),
  mainCommits: [{ sha: source }],
  runs: [run(1, 30, 40)],
  isAncestor: () => true,
  completedAt: at(50),
  windowStart: at(0),
  windowEnd: at(60),
  ...extra,
});

test('HS2-34PWSK parses claims and sanitizes prose into exact operation identifiers', () => {
  const text = `---\nslug: HS2-SYN001\nstatus: completed\nclaim_history:\n- id: c1\n  kind: claim\n  worker: w1\n  at: ${iso(2)}\n---\n<!-- hotsheet:note:begin n1 created_at: ${iso(1)} actor_id_hex: 726f6f74 -->\n## Worker assignment\nOperation \`op-1\` private synthetic prose op-10.\n<!-- hotsheet:note:end -->`;
  const parsed = parseTicket(text),
    safe = evidenceTicket(parsed, ['op-1', 'op-10']);
  assert.equal(safe.claims[0].at, at(2));
  assert.deepEqual(safe.notes[0].operations, ['op-1']);
  assert.equal(JSON.stringify(safe).includes('private'), false);
  assert.equal(
    JSON.stringify(runEvidence({ ...run(1, 30, 40), displayTitle: 'private synthetic prose' })).includes('private'),
    false,
  );
});

test('HS2-34PWSK exact source, required push and first completed green produce verified completion', () => {
  const t = assignmentTimeline(args({ runs: [run(1, 30, 48), run(2, 35, 40)] }));
  assert.equal(t.integratedSha, source);
  assert.equal(t.integratedAt, at(30));
  assert.equal(t.ciCoveredAt, at(40));
  assert.equal(t.coveringRun, 2);
  assert.equal(t.verifiedAt, at(50));
  assert.equal(t.integratorClaimAt, undefined);
  assert.equal(t.mainGateStartAt, undefined);
  assert.equal(t.cohort, 'unknown');
});

test('HS2-4273QB durable assignment transitions replace missing note proxies and expose review time', () => {
  const transitions = [
    { kind: 'offered', at: iso(0), actor: { role: 'dispatcher', id: 'd' } },
    { kind: 'accepted', at: iso(2), actor: { role: 'worker', id: 'w1' } },
    { kind: 'handoff', at: iso(18), actor: { role: 'worker', id: 'w1' } },
    { kind: 'integrating', at: iso(20), actor: { role: 'integrator', id: 'i' } },
    { kind: 'ci', at: iso(30), actor: { role: 'integrator', id: 'i' } },
    { kind: 'integrating', at: iso(31), actor: { role: 'integrator', id: 'i' } },
    { kind: 'ci', at: iso(32), actor: { role: 'integrator', id: 'i' } },
    { kind: 'done', at: iso(50), actor: { role: 'integrator', id: 'i' } },
  ];
  const t = assignmentTimeline(
    args({ assignment: { ...assignment, transitions }, ticket: { ...ticket(), notes: [], claims: [] } }),
  );
  assert.equal(t.assignedAt, at(0));
  assert.equal(t.acceptedAt, at(2));
  assert.equal(t.claimAt, undefined, 'assignment acceptance is not proof of the ticket claim');
  assert.equal(t.handoffAt, at(18));
  assert.equal(t.integratingAt, at(20));
  assert.equal(t.ciStartAt, at(30));
  assert.equal(t.doneAt, at(50));
  assert.equal(t.integratedSha, source);
  assert.equal(t.verifiedAt, at(50));

  const returned = assignmentTimeline(
    args({
      assignment: {
        ...assignment,
        phase: 'blocked',
        transitions: [...transitions.slice(0, 3), { kind: 'review_return', at: iso(21) }],
      },
      ticket: { ...ticket(), notes: [], claims: [] },
    }),
  );
  assert.equal(returned.reviewReturnAt, at(21));
  assert.equal(returned.integratedAt, undefined);
  assert.equal(returned.completedAt, undefined);

  const outside = assignmentTimeline(
    args({
      assignment: { ...assignment, transitions: [{ kind: 'offered', at: iso(-1) }] },
      ticket: ticket(),
    }),
  );
  assert.equal(outside.assignedAt, at(-1));
  assert.equal(outside.handoffAt, undefined);
});

test('HS2-34PWSK same-slug subjects and unproven cherry-picks never establish source integration', () => {
  const unrelated = { sha: other, subject: 'HS2-SYN001 same slug', at: at(30) };
  assert.equal(assignmentTimeline(args({ mainCommits: [unrelated] })).integratedSha, undefined);
  assert.equal(
    assignmentTimeline(args({ mainCommits: [{ ...unrelated, verifiedSource: source }] })).integratedSha,
    other,
  );
  assert.equal(
    assignmentTimeline(args({ mainCommits: [{ sha: source }, { sha: other, verifiedSource: source }] })).integratedSha,
    undefined,
  );
  assert.equal(assignmentTimeline(args({ isAncestor: () => false })).integratedAt, undefined);
});

test('HS2-34PWSK returned original and another-worker correction cannot borrow each other’s evidence', () => {
  const t = ticket();
  t.notes.push(
    { id: 'r', at: at(21), kind: 'return', operations: ['op-1'], commits: [] },
    { id: 'a2', at: at(24), kind: 'assignment', operations: ['op-2'], commits: [] },
    { id: 'h2', at: at(29), kind: 'code', operations: [], commits: [other] },
  );
  t.claims.push({ kind: 'release', worker: 'w1', at: at(31) });
  const original = assignmentTimeline(args({ ticket: t, assignment: { ...assignment, phase: 'blocked' } }));
  assert.equal(original.reviewReturnAt, at(21));
  assert.equal(original.releaseAt, at(20));
  assert.equal(original.integratedAt, undefined);
  assert.equal(original.completedAt, undefined);
  const missingReturn = { ...t, notes: t.notes.filter((n) => n.kind !== 'return') };
  assert.equal(
    assignmentTimeline(args({ ticket: missingReturn, assignment: { ...assignment, phase: 'blocked' } })).integratedAt,
    undefined,
  );
  const corrected = assignmentTimeline(
    args({
      ticket: t,
      assignment: {
        ...assignment,
        id: 'op-2',
        worker: 'correction',
        commit: other,
        handoff: { kind: 'code', commit: other },
      },
      mainCommits: [{ sha: other }],
    }),
  );
  assert.equal(corrected.claimAt, at(25));
  assert.equal(corrected.handoffAt, at(29));
  assert.equal(corrected.integratorClaimAt, undefined);
  assert.equal(
    assignmentTimeline(args({ ticket: t, assignment: { ...assignment, id: 'op-missing' } })).claimAt,
    undefined,
  );
});

test('HS2-34PWSK missing, duplicate or mismatched handoffs and missing addressed notes remain unknown', () => {
  for (const mutate of [
    (t) => t.notes.pop(),
    (t) => t.notes.push({ ...t.notes[1], id: 'dup' }),
    (t) => (t.notes[1].commits = [other]),
    (t) => t.notes.shift(),
  ]) {
    const t = ticket();
    mutate(t);
    const result = assignmentTimeline(args({ ticket: t }));
    assert.equal(result.handoffAt, undefined);
    assert.equal(result.verifiedAt, undefined);
  }
});

test('HS2-34PWSK findings completion uses the typed note id and never counts as shipped code', () => {
  const t = ticket();
  t.notes[1].kind = 'findings';
  const result = assignmentTimeline(
    args({ ticket: t, assignment: { ...assignment, commit: undefined, handoff: { kind: 'findings', note_id: 'n2' } } }),
  );
  assert.equal(result.completedAt, at(50));
  assert.equal(result.integratedAt, undefined);
  const s = summarize([result], { windowStart: at(0), windowEnd: at(60) });
  assert.equal(s.completedFindings, 1);
  assert.equal(s.verifiedShippedCompletedTickets, 0);
});

test('HS2-34PWSK failed, pending, nonrequired and out-of-window CI cannot verify throughput', () => {
  for (const extra of [
    { conclusion: 'failure' },
    { status: 'in_progress' },
    { headBranch: 'feature' },
    { event: 'pull_request' },
    { workflowName: 'Other' },
    { databaseId: '1' },
    { updatedAt: iso(65) },
  ]) {
    const r = run(1, 30, 40, extra),
      t = assignmentTimeline(args({ runs: [r] }));
    assert.equal(t.verifiedAt, undefined);
  }
  assert.equal(requiredRun(run(1, 30, 40)), true);
  assert.equal(assignmentTimeline(args({ completedAt: at(39) })).verifiedAt, undefined);
  assert.equal(assignmentTimeline(args({ assignment: { ...assignment, phase: 'ci' } })).verifiedAt, undefined);
});

test('HS2-34PWSK bounds every joined event and excludes unknown or outside assignment cohorts', () => {
  const t = ticket();
  t.claims[0].at = at(61);
  assert.equal(assignmentTimeline(args({ ticket: t })).claimAt, undefined);
  assert.equal(assignmentTimeline(args({ windowEnd: at(18) })).handoffAt, undefined);
  assert.equal(assignmentTimeline(args({ completedAt: at(61) })).completedAt, undefined);
  const rows = [assignmentTimeline(args()), assignmentTimeline(args({ windowStart: at(3) })), { id: 'missing' }];
  const s = summarize(rows, { windowStart: at(0), windowEnd: at(60) });
  // The second timeline retains a known assignedAt but has no joined window evidence.
  assert.equal(s.verifiedShippedCompletedTickets, 1);
  assert.equal(s.verifiedTicketsPerHour, 1);
  assert.equal(s.excludedUnknownOrOutsideWindow, 1);
  assert.equal(summarize([], { windowStart: at(0), windowEnd: at(60) }).assignments, 0);
  assert.throws(() => summarize([], { windowStart: Infinity, windowEnd: at(60) }));
});

test('HS2-34PWSK gaps use the same worker generation and include missing release opportunities', () => {
  assert.deepEqual(
    workerIdleGaps([
      { worker: 'w', generation: 'g', claimAt: 1, releaseAt: 3 },
      { worker: 'w', generation: 'g', claimAt: 5 },
      { worker: 'w', generation: 'g', claimAt: 9 },
      { worker: 'w', generation: 'new', claimAt: 10 },
    ]),
    [2, undefined],
  );
  assert.equal(distribution([-1, undefined, 2]).unknown, 2);
});

test('HS2-34PWSK frozen identifier-only inputs replay without a CLI, Git or network', () => {
  const input = {
    assignments: [assignment],
    tickets: { 'HS2-SYN001': ticket() },
    completed: { 'HS2-SYN001': at(50) },
    mainCommits: [{ sha: source }],
    runs: [run(1, 30, 40)],
    ancestry: { [`${source}:${head}`]: true },
  };
  const output = measure(JSON.parse(JSON.stringify(input)), { windowStart: at(0), windowEnd: at(60) });
  assert.equal(output.summary.verifiedShippedCompletedTickets, 1);
  assert.deepEqual(
    measure({ ...input, assignments: [], tickets: {} }, { windowStart: at(0), windowEnd: at(60) }).timelines,
    [],
  );
});

test('HS2-34PWSK collector verifies a real cherry-pick independently of same-slug subjects', () => {
  const dir = mkdtempSync(join(tmpdir(), 'hs2-34pwsk-git-'));
  const git = (...args) =>
    execFileSync('git', ['-C', dir, ...args], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
  try {
    git('init', '-b', 'main');
    git('config', 'commit.gpgsign', 'false');
    git('config', 'core.hooksPath', join(dir, 'no-hooks'));
    git('config', 'user.name', 'Synthetic');
    git('config', 'user.email', 'synthetic@example.invalid');
    writeFileSync(join(dir, 'one'), 'base');
    git('add', 'one');
    git('commit', '-m', 'base');
    git('checkout', '-b', 'worker');
    writeFileSync(join(dir, 'one'), 'changed');
    git('commit', '-am', 'HS2-SYN001 source');
    const source = git('rev-parse', 'HEAD');
    git('checkout', 'main');
    writeFileSync(join(dir, 'two'), 'unrelated');
    git('add', 'two');
    git('commit', '-m', 'HS2-SYN001 unrelated');
    const unrelated = git('rev-parse', 'HEAD');
    git('cherry-pick', '-x', source);
    const integrated = git('rev-parse', 'HEAD');
    assert.notEqual(source, integrated);
    const mappings = mapMainCommits(dir, [source, 'd'.repeat(40)], 'main');
    assert.deepEqual(mappings, [{ sha: integrated, verifiedSource: source }]);
    assert.equal(
      mappings.some((row) => row.sha === unrelated),
      false,
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
