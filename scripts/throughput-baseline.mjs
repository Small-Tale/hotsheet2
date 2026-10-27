#!/usr/bin/env node
// Integrator throughput baseline (HS2-34PWSK): joins durable coordination assignments, ticket claim
// history and handoff notes, main-branch Git history, and main CI runs into per-assignment spans.
// Everything is read through existing interfaces; a span whose evidence is missing stays unknown.
//
//   node scripts/throughput-baseline.mjs -C <store> --project <checkout> --cli <verified-cli>
//     --since <RFC 3339> --until <RFC 3339> [--snapshot-out <file>] [--runs <file>] [--json]
//   node scripts/throughput-baseline.mjs --input <snapshot> --since <RFC 3339> --until <RFC 3339>
//
// Without --runs it reads `gh run list --branch main`. It never writes to the store or the pool.

import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const NOTE = /<!-- hotsheet:note:begin (\S+) ([^>]*?)-->\n?([\s\S]*?)<!-- hotsheet:note:end -->/g;
const HANDOFF = /^## (Integration|Findings) handoff\b/m;
const ASSIGNMENT = /^## Worker (?:correction )?assignment\b/m;
const REVIEW_RETURN = /^## Review return\b/m;

function attribute(attributes, name) {
  return new RegExp(`(?:^|\\s)${name}: (\\S+)`).exec(attributes)?.[1];
}

/** Claim history and dated notes from one stored ticket file (`hotsheet-cli show`). */
export function parseTicket(markdown) {
  const frontmatter = /^---\n([\s\S]*?)\n---/.exec(markdown)?.[1] ?? '';
  const claims = [];
  const history = /^claim_history:\n((?:[- ] .*\n?)*)/m.exec(frontmatter)?.[1] ?? '';
  for (const entry of history.split(/^- /m).slice(1)) {
    const field = (name) => new RegExp(`(?:^|\\n)\\s*${name}: (.+)`).exec(entry)?.[1]?.trim();
    const kind = field('kind'),
      worker = field('worker'),
      at = field('at');
    if (kind && worker && at) claims.push({ kind, worker, at: Date.parse(at) });
  }
  const notes = [];
  for (const [, id, attributes, body] of markdown.matchAll(NOTE)) {
    const hex = attribute(attributes, 'actor_id_hex');
    notes.push({
      id,
      createdAt: Date.parse(attribute(attributes, 'created_at') ?? ''),
      actorId: hex ? Buffer.from(hex, 'hex').toString('utf8') : undefined,
      text: body,
    });
  }
  return {
    slug: /^slug: (\S+)/m.exec(frontmatter)?.[1],
    title: /^title: (.+)$/m.exec(frontmatter)?.[1]?.trim(),
    status: /^status: (\S+)/m.exec(frontmatter)?.[1],
    claims: claims.sort((a, b) => a.at - b.at),
    notes: notes.sort((a, b) => a.createdAt - b.createdAt),
  };
}

const finite = Number.isFinite;
const token = (text, value) => text.includes(`\`${value}\``) || text.split(/\s+/).includes(value);
const SHAS = /\b[a-f0-9]{40}\b/g;

/** Strip note prose before archiving: only identifiers, timestamps and transition kinds survive. */
export function evidenceTicket(ticket, operationIds) {
  return {
    slug: ticket.slug,
    status: ticket.status,
    claims: ticket.claims,
    notes: ticket.notes
      .map((note) => ({
        id: note.id,
        at: note.createdAt,
        actorId: note.actorId,
        kind: ASSIGNMENT.test(note.text)
          ? 'assignment'
          : REVIEW_RETURN.test(note.text)
            ? 'return'
            : /^## Findings handoff\b/m.test(note.text)
              ? 'findings'
              : HANDOFF.test(note.text)
                ? 'code'
                : 'other',
        operations: operationIds.filter((id) => token(note.text, id)),
        commits: [...new Set(note.text.match(SHAS) ?? [])],
      }))
      .filter((note) => note.kind !== 'other'),
  };
}

export function runEvidence(run) {
  const { databaseId, headSha, headBranch, createdAt, updatedAt, conclusion, status, event, workflowName } = run;
  return { databaseId, headSha, headBranch, createdAt, updatedAt, conclusion, status, event, workflowName };
}

export function requiredRun(run) {
  return (
    run.workflowName === 'CI' &&
    run.headBranch === 'main' &&
    run.event === 'push' &&
    /^[a-f0-9]{40}$/.test(run.headSha) &&
    Number.isInteger(run.databaseId) &&
    finite(Date.parse(run.createdAt))
  );
}

/** Bounded assignment-note proxies; absent or ambiguous evidence remains unknown. */
export function assignmentTimeline({
  assignment,
  ticket,
  mainCommits,
  runs,
  isAncestor,
  completedAt,
  windowStart = -Infinity,
  windowEnd = Infinity,
}) {
  const addressed = ticket.notes.filter((n) => n.kind === 'assignment' && n.operations.includes(assignment.id));
  const assignedAt = addressed.length === 1 ? addressed[0].at : undefined;
  const next = ticket.notes.find((n) => n.kind === 'assignment' && n.at > assignedAt)?.at ?? Infinity;
  const within = (at) => finite(at) && at >= assignedAt && at < next && at >= windowStart && at <= windowEnd;
  const unknown = {
    id: assignment.id,
    slug: ticket.slug,
    worker: assignment.worker,
    generation: assignment.generation,
    qualifiedTicket: assignment.ticket,
    phase: assignment.phase,
    assignedAt,
    cohort: 'unknown',
    risk: 'unknown',
    scope: 'unknown',
    overlap: 'unknown',
    platform: 'unknown',
    provider: 'unknown',
  };
  if (!within(assignedAt)) return unknown;
  const claim = ticket.claims.find((c) => within(c.at) && c.kind === 'claim' && c.worker === assignment.worker);
  const kind = assignment.handoff?.kind ?? (assignment.commit ? 'code' : undefined);
  const source =
    assignment.handoff?.kind === 'code' && assignment.handoff.commit === assignment.commit
      ? assignment.commit
      : undefined;
  const matches = ticket.notes.filter(
    (n) =>
      within(n.at) &&
      n.at >= claim?.at &&
      (kind === 'findings'
        ? n.kind === 'findings' && n.id === assignment.handoff.note_id
        : kind === 'code' && n.kind === 'code' && n.commits.includes(source)),
  );
  const handoff = matches.length === 1 ? matches[0] : undefined;
  const returned = ticket.notes.find(
    (n) => within(n.at) && n.at >= handoff?.at && n.kind === 'return' && n.operations.includes(assignment.id),
  );
  const release = ticket.claims.find(
    (c) => within(c.at) && c.at >= handoff?.at && c.kind === 'release' && c.worker === assignment.worker,
  );
  // Git identity or an independently computed unique stable patch-id mapping; never subject matching.
  const candidates = source ? mainCommits.filter((c) => c.sha === source || c.verifiedSource === source) : [];
  const integrated =
    !returned && assignment.phase !== 'blocked' && kind === 'code' && handoff && candidates.length === 1
      ? candidates[0]
      : undefined;
  const containing = integrated
    ? runs.filter(
        (r) =>
          requiredRun(r) &&
          within(Date.parse(r.createdAt)) &&
          Date.parse(r.createdAt) >= handoff.at &&
          isAncestor(integrated.sha, r.headSha),
      )
    : [];
  const pushed = containing.toSorted((a, b) => Date.parse(a.createdAt) - Date.parse(b.createdAt))[0];
  const green = containing
    .filter(
      (r) =>
        r.status === 'completed' &&
        r.conclusion === 'success' &&
        within(Date.parse(r.updatedAt)) &&
        Date.parse(r.updatedAt) >= Date.parse(r.createdAt),
    )
    .sort((a, b) => Date.parse(a.updatedAt) - Date.parse(b.updatedAt))[0];
  const completed =
    !returned &&
    assignment.phase === 'done' &&
    ticket.status === 'completed' &&
    handoff &&
    within(completedAt) &&
    completedAt >= handoff.at
      ? completedAt
      : undefined;
  return {
    ...unknown,
    kind,
    sourceCommit: source,
    claimAt: claim?.at,
    handoffAt: handoff?.at,
    releaseAt: release?.at,
    reviewReturnAt: returned?.at,
    integratedSha: integrated?.sha,
    integrationEvidence: integrated
      ? integrated.sha === source
        ? 'git-identity'
        : 'unique-stable-patch-id'
      : undefined,
    integratedAt: pushed ? Date.parse(pushed.createdAt) : undefined,
    ciCoveredAt: green ? Date.parse(green.updatedAt) : undefined,
    coveringRun: green?.databaseId,
    completedAt: completed,
    verifiedAt: green && completed >= Date.parse(green.updatedAt) ? completed : undefined,
  };
}

/** Observed release-to-next-own-claim gap; neither availability nor idle capacity is inferred. */
export function workerIdleGaps(timelines) {
  const groups = Map.groupBy(
    timelines.filter((t) => finite(t.claimAt)),
    (t) => `${t.worker}:${t.generation}`,
  );
  const gaps = [];
  for (const items of groups.values()) {
    items.sort((a, b) => a.claimAt - b.claimAt);
    for (let i = 1; i < items.length; i++)
      gaps.push(finite(items[i - 1].releaseAt) ? items[i].claimAt - items[i - 1].releaseAt : undefined);
  }
  return gaps;
}

export function distribution(values) {
  const known = values
    .filter((value) => value !== undefined && Number.isFinite(value) && value >= 0)
    .sort((a, b) => a - b);
  const at = (fraction) => known[Math.min(known.length - 1, Math.floor(fraction * known.length))];
  return {
    n: known.length,
    unknown: values.length - known.length,
    median: known.length ? at(0.5) : undefined,
    p75: known.length ? at(0.75) : undefined,
    p90: known.length ? at(0.9) : undefined,
  };
}

const span = (timelines, from, to) =>
  timelines.map((timeline) =>
    timeline[from] === undefined || timeline[to] === undefined ? undefined : timeline[to] - timeline[from],
  );

/** Aggregate spans, throughput and correction rate over one observation window. */
export function summarize(timelines, { windowStart, windowEnd }) {
  if (!finite(windowStart) || !finite(windowEnd) || windowEnd <= windowStart)
    throw new Error('Invalid observation window');
  const inWindow = (t) => finite(t.assignedAt) && t.assignedAt >= windowStart && t.assignedAt <= windowEnd;
  const cohort = timelines.filter(inWindow);
  const handedOff = cohort.filter((t) => finite(t.handoffAt));
  const integrated = cohort.filter((t) => finite(t.integratedAt));
  const verified = integrated.filter((t) => t.kind === 'code' && finite(t.verifiedAt) && t.verifiedAt <= windowEnd);
  const hours = (windowEnd - windowStart) / 3_600_000;
  const countTickets = (rows) =>
    new Set(
      rows.map((t) =>
        t.qualifiedTicket ? `${t.qualifiedTicket.connection_id}:${t.qualifiedTicket.native_id}` : t.slug,
      ),
    ).size;
  return {
    window: { start: new Date(windowStart).toISOString(), end: new Date(windowEnd).toISOString(), hours },
    assignments: cohort.length,
    excludedUnknownOrOutsideWindow: timelines.length - cohort.length,
    handoffs: handedOff.length,
    findingsHandoffs: handedOff.filter((t) => t.kind === 'findings').length,
    completedFindings: cohort.filter((t) => t.kind === 'findings' && finite(t.completedAt)).length,
    reviewReturns: cohort.filter((t) => finite(t.reviewReturnAt)).length,
    integratedCodeAssignments: integrated.length,
    verifiedShippedCompletedTickets: countTickets(verified),
    integratedPendingVerification: integrated.filter((t) => !finite(t.verifiedAt)).length,
    verifiedTicketsPerHour: countTickets(verified) / hours,
    spans: {
      assignedToClaim: distribution(span(cohort, 'assignedAt', 'claimAt')),
      claimToHandoff: distribution(span(cohort, 'claimAt', 'handoffAt')),
      handoffToIntegratorClaim: distribution(span(handedOff, 'handoffAt', 'integratorClaimAt')),
      integratingToMainGate: distribution(span(handedOff, 'integratingAt', 'mainGateStartAt')),
      mainGate: distribution(span(handedOff, 'mainGateStartAt', 'mainGateEndAt')),
      handoffToFirstObservedPush: distribution(span(handedOff, 'handoffAt', 'integratedAt')),
      firstObservedPushToCiGreen: distribution(span(integrated, 'integratedAt', 'ciCoveredAt')),
      ciGreenToCompleted: distribution(span(integrated, 'ciCoveredAt', 'completedAt')),
      assignedToVerifiedCompletion: distribution(span(verified, 'assignedAt', 'verifiedAt')),
      observedReleaseToNextClaim: distribution(workerIdleGaps(cohort)),
    },
  };
}

function minutes(value) {
  return value === undefined ? 'unknown' : `${(value / 60_000).toFixed(1)}m`;
}

export function formatSummary(summary) {
  const lines = [
    `Window ${summary.window.start} to ${summary.window.end} (${summary.window.hours.toFixed(1)} h)`,
    `Assignments ${summary.assignments}; handoffs ${summary.handoffs} (findings ${summary.findingsHandoffs}); review returns ${summary.reviewReturns}; integrated code assignments ${summary.integratedCodeAssignments}`,
    `Verified shipped completed tickets ${summary.verifiedShippedCompletedTickets}; per hour ${summary.verifiedTicketsPerHour.toFixed(2)}; integrated pending verification ${summary.integratedPendingVerification}`,
    `Excluded unknown/outside-window assignments ${summary.excludedUnknownOrOutsideWindow}; cohort dimensions unknown`,
    'span                         n  unknown   median      p75      p90',
  ];
  for (const [name, value] of Object.entries(summary.spans))
    lines.push(
      `${name.padEnd(26)} ${String(value.n).padStart(3)} ${String(value.unknown).padStart(8)} ${minutes(value.median).padStart(8)} ${minutes(value.p75).padStart(8)} ${minutes(value.p90).padStart(8)}`,
    );
  return lines.join('\n');
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024, ...options });
  if (result.status !== 0) throw new Error(`${command} ${args.join(' ')} failed: ${result.stderr?.trim()}`);
  return result.stdout;
}

/** Independently verify source identity or unique stable patch equivalence against main history. */
export function mapMainCommits(project, sources, mainRef = 'origin/main') {
  const hashes = run('git', ['-C', project, 'log', mainRef, '--format=%H', '-n', '2000'])
    .trim()
    .split('\n')
    .filter(Boolean);
  const patchId = (sha) => {
    const result = spawnSync('git', ['-C', project, 'show', '--pretty=format:', '--no-ext-diff', sha], {
      encoding: 'utf8',
      maxBuffer: 256 * 1024 * 1024,
    });
    if (result.status !== 0) return undefined;
    return run('git', ['patch-id', '--stable'], { input: result.stdout }).split(/\s/)[0] || undefined;
  };
  const sourcePatches = new Map(sources.map((sha) => [sha, patchId(sha)]));
  return hashes
    .map((sha) => {
      if (sources.includes(sha)) return { sha };
      const patch = patchId(sha);
      const matches = sources.filter((source) => patch && sourcePatches.get(source) === patch);
      return { sha, ...(matches.length === 1 ? { verifiedSource: matches[0] } : {}) };
    })
    .filter((c) => sources.includes(c.sha) || c.verifiedSource);
}

export function measure(input, window) {
  const timelines = input.assignments.map((assignment) =>
    assignmentTimeline({
      assignment,
      ticket: input.tickets[assignment.slug],
      mainCommits: input.mainCommits,
      runs: input.runs,
      completedAt: input.completed[assignment.slug],
      ...window,
      isAncestor: (a, b) => input.ancestry[`${a}:${b}`] === true,
    }),
  );
  return { summary: summarize(timelines, window), timelines };
}

function main(argv) {
  const option = (name) => {
    const i = argv.indexOf(name);
    return i < 0 ? undefined : argv[i + 1];
  };
  const window = { windowStart: Date.parse(option('--since')), windowEnd: Date.parse(option('--until')) };
  if (!finite(window.windowStart) || !finite(window.windowEnd) || window.windowEnd <= window.windowStart)
    throw new Error('Explicit valid --since and --until are required');
  let input;
  if (option('--input')) input = JSON.parse(readFileSync(option('--input'), 'utf8'));
  else {
    const store = option('-C'),
      project = option('--project'),
      cli = option('--cli');
    if (!store || !project || !cli) throw new Error('Collection requires -C, --project and explicit verified --cli');
    const status = JSON.parse(run(cli, ['-C', store, 'coordination', '--project', project, 'status']));
    const assignments = Object.values(status.state.assignments);
    const tickets = {};
    for (const slug of new Set(assignments.map((a) => a.slug)))
      tickets[slug] = evidenceTicket(
        parseTicket(run(cli, ['-C', store, 'show', slug])),
        assignments.filter((a) => a.slug === slug).map((a) => a.id),
      );
    const phases = JSON.parse(run(cli, ['-C', store, 'phase-report', '--json']));
    const completed = Object.fromEntries(phases.tickets.map((t) => [t.slug, Date.parse(t.completed_at)]));
    const runs = JSON.parse(
      option('--runs')
        ? readFileSync(option('--runs'), 'utf8')
        : run(
            'gh',
            [
              'run',
              'list',
              '--branch',
              'main',
              '--limit',
              '200',
              '--json',
              'databaseId,headSha,headBranch,createdAt,updatedAt,conclusion,status,event,workflowName',
            ],
            { cwd: project },
          ),
    );
    const mainCommits = mapMainCommits(project, [...new Set(assignments.map((a) => a.commit).filter(Boolean))]);
    const ancestry = {};
    for (const commit of mainCommits)
      for (const r of runs)
        if (requiredRun(r)) {
          const result = spawnSync('git', ['-C', project, 'merge-base', '--is-ancestor', commit.sha, r.headSha]);
          if (result.status === 0 || result.status === 1) ancestry[`${commit.sha}:${r.headSha}`] = result.status === 0;
        }
    input = {
      version: 1,
      capturedAt: new Date().toISOString(),
      assignments: assignments.map((a) => ({
        id: a.id,
        slug: a.slug,
        ticket: a.ticket,
        worker: a.worker,
        generation: a.generation,
        phase: a.phase,
        commit: a.commit,
        handoff: a.handoff,
      })),
      tickets,
      completed,
      runs: runs.map(runEvidence),
      mainCommits,
      ancestry,
    };
    if (option('--snapshot-out'))
      writeFileSync(option('--snapshot-out'), JSON.stringify(input, null, 2) + '\n', { mode: 0o600 });
  }
  if (input.version !== 1 || !finite(Date.parse(input.capturedAt)) || window.windowEnd > Date.parse(input.capturedAt))
    throw new Error('Snapshot version/capture time invalid or observation ends after capture');
  const result = measure(input, window);
  console.log(argv.includes('--json') ? JSON.stringify(result, null, 2) : formatSummary(result.summary));
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
