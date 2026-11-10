#!/usr/bin/env node
// HS2-6QEZB0: bounded, privacy-safe review of observed gate outcomes and costs.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const safe = (value) =>
  typeof value === 'string' &&
  value.length <= 300 &&
  !/[\\\r\n\t\0]/.test(value) &&
  !/(?:^\/|[A-Za-z]:\/|\/Users\/|\/home\/|\/private\/)/.test(value);
const safeCategory = (value) => safe(value) && /^[a-z0-9:._-]+$/i.test(value);
const outcomes = new Set(['pass', 'fail', 'skip', 'reused', 'interrupted']);
const quantile = (values, q) =>
  values.length ? [...values].sort((a, b) => a - b)[Math.ceil(values.length * q) - 1] : null;
const ms = (value) => (Number.isFinite(value) && value > 0 ? value : null);

function location(id, kind) {
  if (!safe(id)) return null;
  const runner = /^(nextest|vitest|playwright):(.+?)(?:#([a-f0-9]{16}))?$/.exec(id);
  if (runner)
    return {
      area: runner[1],
      stage: runner[1],
      // Nextest case names contain a binary and module path, not a source file.
      file: runner[1] === 'nextest' && kind === 'case' ? 'unknown-source-file' : runner[2],
      caseId: kind === 'case' ? id : null,
    };
  const lint = /^web-lint:(?:doctor:)?(.+)$/.exec(id);
  if (lint) return { area: 'clients/web', stage: id, file: null, caseId: null };
  const gate = /^gate:([^:]+):([a-f0-9]{16})$/.exec(id);
  if (gate) return { area: gate[1], stage: id, file: null, caseId: null };
  const ci = /^ci:([^:]+):(.+)$/.exec(id);
  if (ci) return { area: 'ci', stage: id, file: null, caseId: null };
  if (id.startsWith('report:')) return { area: 'ci-reports', stage: id, file: null, caseId: null };
  return { area: 'other', stage: id, file: null, caseId: null };
}

function distribution(values) {
  return {
    samples: values.length,
    p50: quantile(values, 0.5),
    p75: quantile(values, 0.75),
    p90: quantile(values, 0.9),
    totalMs: values.reduce((sum, value) => sum + value, 0),
  };
}

function metrics(matching, selected, deferred, id, periodMs, newest) {
  const durations = matching
    .filter((row) => row.outcome === 'pass' || row.outcome === 'fail')
    .map((row) => ms(row.ms))
    .filter((value) => value !== null);
  const previous = matching.filter(
    (row) => Number.isFinite(row.at) && row.at < newest - periodMs && row.at >= newest - 2 * periodMs,
  );
  const current = matching.filter((row) => Number.isFinite(row.at) && row.at >= newest - periodMs && row.at <= newest);
  const duration = (items) =>
    distribution(
      items
        .filter((row) => row.outcome === 'pass' || row.outcome === 'fail')
        .map((row) => ms(row.ms))
        .filter((value) => value !== null),
    );
  const categories = Object.fromEntries(
    [...new Set(matching.map((row) => row.category).filter(safeCategory))]
      .sort()
      .map((category) => [category, matching.filter((row) => row.category === category).length]),
  );
  return {
    id,
    selected,
    deferred,
    attempted: matching.length,
    denominator: selected || null,
    pass: matching.filter((row) => row.outcome === 'pass').length,
    fail: matching.filter((row) => row.outcome === 'fail').length,
    skip: matching.filter((row) => row.outcome === 'skip').length,
    reused: matching.filter((row) => row.outcome === 'reused').length,
    interrupted: matching.filter((row) => row.outcome === 'interrupted').length,
    cost: distribution(durations),
    failureCost: duration(matching.filter((row) => row.outcome === 'fail')),
    trend: {
      previous: duration(previous),
      current: duration(current),
      comparable: previous.length >= 2 && current.length >= 2,
    },
    failureCategories: categories,
    unknown: matching.length === 0,
    sparse: matching.length < 5,
  };
}

function sorted(items, sort) {
  const key = (item) => (sort === 'failures' ? item.metrics.fail : sort === 'cost' ? item.metrics.cost.totalMs : 0);
  return items.sort((a, b) =>
    sort === 'name' ? a.id.localeCompare(b.id) : key(b) - key(a) || a.id.localeCompare(b.id),
  );
}

export function buildGateReview(history, { source, os, cohort, area, sort = 'cost', periodDays = 7 } = {}) {
  if (!['cost', 'failures', 'name'].includes(sort) || !Number.isFinite(periodDays) || periodDays <= 0)
    throw new Error('Invalid review sort or period');
  const attempts = [
    ...new Map(
      (history.attempts ?? [])
        .filter(
          (attempt) =>
            ['local', 'ci'].includes(attempt.source) &&
            safe(attempt.id) &&
            safe(attempt.cohort) &&
            safe(attempt.os) &&
            (!source || attempt.source === source) &&
            (!os || attempt.os === os) &&
            (!cohort || attempt.cohort === cohort),
        )
        .map((attempt) => [attempt.id, attempt]),
    ).values(),
  ];
  const periodMs = periodDays * 86_400_000;
  const cohorts = [...new Set(attempts.map((attempt) => JSON.stringify([attempt.source, attempt.os, attempt.cohort])))]
    .sort()
    .map((key) => {
      const [cohortSource, cohortOs, cohortName] = JSON.parse(key);
      const subset = attempts.filter(
        (attempt) => attempt.source === cohortSource && attempt.os === cohortOs && attempt.cohort === cohortName,
      );
      const newest = Math.max(
        0,
        ...subset.map((attempt) => attempt.endedAt ?? attempt.startedAt).filter(Number.isFinite),
      );
      const rows = subset.flatMap((attempt) =>
        [
          ...(attempt.checks ?? []).map((check) => ({ ...check, kind: 'check' })),
          ...(attempt.cases ?? []).map((check) => ({ ...check, kind: 'case' })),
        ]
          .filter((check) => safe(check.id) && outcomes.has(check.outcome))
          .map((check) => ({
            ...check,
            at: attempt.endedAt ?? attempt.startedAt,
            runId: attempt.runId,
            runAttempt: attempt.runAttempt,
          })),
      );
      const byId = new Map();
      const byArea = new Map();
      const byStage = new Map();
      const push = (map, id, row) => {
        const existing = map.get(id) ?? [];
        existing.push(row);
        map.set(id, existing);
      };
      for (const row of rows) {
        const path = location(row.id, row.kind);
        if (!path) continue;
        push(byId, row.id, row);
        if (row.kind === 'check') {
          push(byArea, path.area, row);
          push(byStage, path.stage, row);
        }
      }
      const selections = (field) => {
        const counts = { id: new Map(), area: new Map(), stage: new Map() };
        for (const attempt of subset) {
          const perAttempt = { id: new Set(), area: new Set(), stage: new Set() };
          for (const id of attempt[field] ?? []) {
            const path = location(id, 'check');
            if (!path) continue;
            perAttempt.id.add(id);
            perAttempt.area.add(path.area);
            perAttempt.stage.add(path.stage);
          }
          for (const level of ['id', 'area', 'stage'])
            for (const id of perAttempt[level]) counts[level].set(id, (counts[level].get(id) ?? 0) + 1);
        }
        return counts;
      };
      const selected = selections('selected');
      const deferred = selections('deferred');
      const areas = new Map();
      const add = (id, kind) => {
        const path = location(id, kind);
        if (!path || (area && path.area !== area)) return;
        const areaNode = areas.get(path.area) ?? { id: path.area, stages: new Map() };
        areas.set(path.area, areaNode);
        const stage = areaNode.stages.get(path.stage) ?? { id: path.stage, files: new Map() };
        areaNode.stages.set(path.stage, stage);
        if (path.file) {
          const file = stage.files.get(path.file) ?? { id: path.file, cases: new Set() };
          stage.files.set(path.file, file);
          if (path.caseId) file.cases.add(path.caseId);
        }
      };
      for (const attempt of subset)
        for (const id of [...(attempt.selected ?? []), ...(attempt.deferred ?? [])]) add(id, 'check');
      for (const row of rows) add(row.id, row.kind);
      const decorate = (id, level) => {
        const rows = level === 'area' ? byArea.get(id) : level === 'stage' ? byStage.get(id) : byId.get(id);
        const counter = level === 'area' || level === 'stage' ? level : 'id';
        return {
          id,
          metrics: metrics(
            rows ?? [],
            selected[counter].get(id) ?? 0,
            deferred[counter].get(id) ?? 0,
            id,
            periodMs,
            newest,
          ),
        };
      };
      const nested = sorted(
        [...areas.values()].map((areaNode) => ({
          ...decorate(areaNode.id, 'area'),
          stages: sorted(
            [...areaNode.stages.values()].map((stage) => ({
              ...decorate(stage.id, 'stage'),
              label: stage.id.startsWith('gate:') ? null : stage.id,
              files: sorted(
                [...stage.files.values()].map((file) => ({
                  ...decorate(`${stage.id}:${file.id}`, 'file'),
                  cases: sorted(
                    [...file.cases].map((id) => decorate(id, 'case')),
                    sort,
                  ),
                })),
                sort,
              ),
            })),
            sort,
          ),
        })),
        sort,
      );
      return {
        source: cohortSource,
        os: cohortOs,
        cohort: cohortName,
        attempts: subset.length,
        retries: subset.filter((attempt) => Number(attempt.runAttempt) > 1).length,
        observedRecoveries: [
          ...new Set(
            rows.filter((row) => row.runId && row.outcome === 'fail').map((row) => JSON.stringify([row.runId, row.id])),
          ),
        ].filter((candidate) => {
          const [runId, id] = JSON.parse(candidate);
          const related = byId.get(id) ?? [];
          const firstFailure = Math.min(
            ...related
              .filter((row) => row.runId === runId && row.outcome === 'fail')
              .map((row) => Number(row.runAttempt)),
          );
          return related.some(
            (row) => row.runId === runId && row.outcome === 'pass' && Number(row.runAttempt) > firstFailure,
          );
        }).length,
        warmCold: 'unknown',
        runnerConcurrency: 'unknown',
        selectedPopulationBias: true,
        sparse: subset.length < 5,
        areas: nested,
      };
    });
  return {
    schemaVersion: 1,
    retainedAttempts: attempts.length,
    periodDays,
    cohorts,
    caveats: [
      'The bounded 20-attempt ledger is selected work, not a representative failure-rate sample.',
      'Runner concurrency and warm/cold state are unknown; reuse is recorded separately.',
      'File and case compute durations may overlap and are never summed as gate wall time.',
      'A fail-to-pass observation is not a population flake rate or proof of productive detection.',
      'Hashed gate-stage labels remain unknown without a safe command mapping; older attempts may lack lint substages.',
    ],
  };
}

export function parseReviewArgs(argv) {
  const options = { format: 'text', sort: 'cost', periodDays: 7 };
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (
      !value ||
      !['--history', '--format', '--sort', '--source', '--os', '--cohort', '--area', '--period-days'].includes(flag)
    )
      throw new Error(
        'Usage: gate-review --history FILE [--format text|json] [--sort cost|failures|name] [--source local|ci] [--os ID] [--cohort ID] [--area ID] [--period-days N]',
      );
    if (flag === '--history') options.history = value;
    else if (flag === '--period-days') options.periodDays = Number(value);
    else options[flag.slice(2)] = value;
  }
  if (
    !options.history ||
    !['text', 'json'].includes(options.format) ||
    !['cost', 'failures', 'name'].includes(options.sort) ||
    (options.source && !['local', 'ci'].includes(options.source)) ||
    !Number.isFinite(options.periodDays) ||
    options.periodDays <= 0
  )
    throw new Error('Invalid gate-review arguments');
  for (const key of ['os', 'cohort', 'area'])
    if (options[key] && !safe(options[key])) throw new Error(`Invalid ${key}`);
  return options;
}

export function formatGateReview(report) {
  const lines = [`Gate review: ${report.retainedAttempts} retained attempts; ${report.periodDays}-day periods`];
  for (const cohort of report.cohorts) {
    lines.push(
      `\n${cohort.source}/${cohort.os}/${cohort.cohort}: ${cohort.attempts} attempts${cohort.sparse ? ' (sparse)' : ''}`,
    );
    for (const area of cohort.areas)
      for (const stage of area.stages) {
        const m = stage.metrics;
        lines.push(
          `  ${area.id} > ${stage.label ?? stage.id}: selected ${m.selected}, deferred ${m.deferred}, pass ${m.pass}, fail ${m.fail}, skip ${m.skip}, reused ${m.reused}, p50/p75/p90 ${m.cost.p50 ?? '?'} / ${m.cost.p75 ?? '?'} / ${m.cost.p90 ?? '?'} ms`,
        );
        for (const file of stage.files) {
          lines.push(
            `    ${file.id}: ${file.metrics.attempted} observed; ${file.metrics.cost.totalMs} compute ms; ${file.cases.length} case ids`,
          );
          for (const testCase of file.cases)
            lines.push(`      ${testCase.id}: ${testCase.metrics.attempted} observed; ${testCase.metrics.fail} failed`);
        }
      }
  }
  lines.push(...report.caveats.map((item) => `Note: ${item}`));
  return lines.join('\n');
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const options = parseReviewArgs(process.argv.slice(2));
    const report = buildGateReview(JSON.parse(readFileSync(options.history, 'utf8')), options);
    console.log(options.format === 'json' ? JSON.stringify(report, null, 2) : formatGateReview(report));
  } catch (error) {
    console.error(`gate-review: ${error.message}`);
    process.exitCode = 1;
  }
}
