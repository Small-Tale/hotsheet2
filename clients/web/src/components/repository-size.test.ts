import { describe, expect, it } from 'vitest';

import type { CompactionPlan, RepositorySizeEntry, RepositorySizeReport } from '../api';
import { parseCompactionPlan, parseRepositorySizeReport, ServerResponseShapeError } from '../server-response-parsers';
import { formatBytes, RepositorySize, repositorySizeEntryTitle } from './repository-size';

const THRESHOLDS = { warning_bytes: 5_000_000_000, critical_bytes: 10_000_000_000 };

function git(total: number) {
  return {
    loose_objects: 2,
    loose_bytes: 1024,
    packed_objects: 1200,
    packs: 1,
    packed_bytes: total,
    total_bytes: total,
  };
}

const code: RepositorySizeEntry = {
  kind: 'code',
  label: 'hotsheet2',
  locator: '/work/hotsheet2',
  status: 'measured',
  git: git(60_600_000),
  shared_with_code: false,
  level: 'ok',
};
const store: RepositorySizeEntry = {
  kind: 'ticket_store',
  connection_id: 'abc123',
  provider: 'git',
  label: 'hotsheet2.hs2',
  locator: '/work/hotsheet2.hs2',
  status: 'measured',
  git: git(1_250_000_000),
  attachments: { bytes: 1_100_000_000, files: 1, truncated: false },
  shared_with_code: false,
  level: 'ok',
};

function report(entries: RepositorySizeEntry[], level: RepositorySizeReport['level'] = 'ok'): RepositorySizeReport {
  return { checkout_id: 'hotsheet2-1a2b3c', thresholds: THRESHOLDS, level, entries };
}

function render(value: RepositorySizeReport) {
  return String(RepositorySize({ state: { status: 'ready', report: value } }));
}

describe('formatBytes (HS2-QF44T7)', () => {
  it('uses decimal units with at most one decimal and no trailing zero', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(999)).toBe('999 B');
    expect(formatBytes(1500)).toBe('1.5 kB');
    expect(formatBytes(60_600_000)).toBe('60.6 MB');
    expect(formatBytes(250_000_000)).toBe('250 MB');
    expect(formatBytes(5_000_000_000)).toBe('5 GB');
    expect(formatBytes(99_960_000)).toBe('100 MB');
    expect(formatBytes(12_340_000_000_000_000)).toBe('12340 TB');
  });
});

describe('RepositorySize (HS2-QF44T7)', () => {
  it('lists the code repository and each store with git and attachment sizes', () => {
    const markup = render(report([code, store]));
    expect(markup).toContain('data-state="ready"');
    expect(markup).toContain('data-level="ok"');
    expect(markup).toContain('Code repository: hotsheet2');
    expect(markup).toContain('Ticket store: hotsheet2.hs2');
    expect(markup).toContain('60.6 MB (1,202 objects)');
    expect(markup).toContain('1.3 GB (1,202 objects)');
    expect(markup).toContain('1.1 GB in 1 file');
    expect(markup).toContain('Within limits');
    expect(markup).toContain('GitHub recommends staying under 5 GB and rejects repositories over 10 GB');
    // Only stores carry an attachments row; every entry names its location.
    expect(markup.match(/Attachments/gu)).toHaveLength(1);
    expect(markup.match(/Location/gu)).toHaveLength(2);
    expect(markup).not.toContain('kui-state-banner');
  });

  it('shows a warning banner naming the store over the soft limit', () => {
    const over = { ...store, level: 'warning' as const, git: git(6_200_000_000) };
    const markup = render(report([code, over], 'warning'));
    expect(markup).toContain('data-level="warning"');
    expect(markup).toContain("Ticket store: hotsheet2.hs2 is over GitHub's 5 GB soft limit.");
    expect(markup).toContain('Over soft limit');
    expect(markup).not.toContain('role="alert"');
  });

  it('shows an alert banner naming every repository over the hard limit', () => {
    const big = { ...code, level: 'critical' as const, git: git(11_000_000_000) },
      bigStore = { ...store, level: 'critical' as const, git: git(12_000_000_000) };
    const markup = render(report([big, bigStore], 'critical'));
    expect(markup).toContain(
      "Code repository: hotsheet2 and Ticket store: hotsheet2.hs2 are over GitHub's 10 GB hard limit.",
    );
    expect(markup).toContain('Over hard limit');
    expect(markup).toContain('GitHub rejects pushes');
  });

  it('explains unsupported, unmeasurable, shared, and truncated entries', () => {
    const github: RepositorySizeEntry = {
      kind: 'ticket_store',
      connection_id: 'github-main',
      provider: 'github',
      label: 'github-main',
      locator: 'Small-Tale/hotsheet2',
      status: 'unsupported',
      shared_with_code: false,
      level: 'ok',
      detail: 'This github source is hosted by its provider, so there is no local repository to measure.',
    };
    const plain: RepositorySizeEntry = {
      ...store,
      status: 'not_repository',
      git: undefined,
      attachments: { bytes: 2048, files: 2, truncated: true },
      detail: 'The directory is not inside a Git repository.',
    };
    const embedded: RepositorySizeEntry = { ...store, label: '.hotsheet', shared_with_code: true };
    const markup = render(report([code, embedded, plain, github]));
    expect(markup).toContain('Not measured');
    expect(markup).toContain('there is no local repository to measure');
    expect(markup).toContain('Not a Git repository');
    expect(markup).toContain('At least 2 kB in 2 files');
    expect(markup).toContain('Shared with the code repository: 1.3 GB');
    expect(markup).toContain('data-status="unsupported"');
  });

  it('renders loading, error, and empty states', () => {
    expect(String(RepositorySize({ state: { status: 'loading' } }))).toContain('Loading Repository Size');
    const error = String(RepositorySize({ state: { status: 'error', message: 'Server unreachable.' } }));
    expect(error).toContain('Repository size is unavailable.');
    expect(error).toContain('Server unreachable.');
    expect(render(report([]))).toContain('Nothing to Measure');
  });

  it('titles entries by kind', () => {
    expect(repositorySizeEntryTitle(code)).toBe('Code repository: hotsheet2');
    expect(repositorySizeEntryTitle(store)).toBe('Ticket store: hotsheet2.hs2');
  });

  it('offers only dedicated Git stores and displays the advisory preview', () => {
    const plan: CompactionPlan = {
      head: 'a'.repeat(40),
      branch: 'main',
      min_age_days: 90,
      oversized_bytes: 10_000_000,
      candidates: [
        {
          ticket_id: 'one',
          ticket_slug: 'HS-ONE',
          attachment_id: 'att',
          filename: 'old.png',
          path: 'attachments/one/att/old.png',
          bytes: 12_000_000,
          sha256: 'b'.repeat(64),
          reason: 'oversized',
        },
      ],
      reclaimable_payload_bytes: 12_000_000,
      fingerprint: 'c'.repeat(64),
    };
    const markup = String(
      RepositorySize({
        state: {
          status: 'ready',
          report: report([code, store, { ...store, connection_id: 'embedded', shared_with_code: true }]),
        },
        compaction: { status: 'ready', source: 'abc123', plan },
      }),
    );
    expect(markup.match(/Preview Attachment Compaction/gu)).toHaveLength(1);
    expect(markup).toContain('HS-ONE: old.png (12 MB, oversized)');
    expect(markup).toContain('12 MB of attachment payload history');
    expect(markup).toContain('New Mirror Backup Path');
    expect(markup).toContain(`rewrite main ${'c'.repeat(64)}`);
    expect(markup).toContain('Back Up and Rewrite History');
    expect(parseCompactionPlan(plan)).toBe(plan);
    expect(() => parseCompactionPlan({ ...plan, candidates: [{ ...plan.candidates[0], reason: 'unknown' }] })).toThrow(
      ServerResponseShapeError,
    );
    const completed = String(
      RepositorySize({
        state: { status: 'ready', report: report([code, store]) },
        compaction: {
          status: 'ready',
          source: 'abc123',
          plan,
          execution: {
            status: 'success',
            result: {
              old_head: 'a'.repeat(40),
              new_head: 'd'.repeat(40),
              backup: '/tmp/backup.git',
              candidates_removed: 1,
              git_bytes_before: 20_000_000,
              git_bytes_after: 8_000_000,
              recovery: ['Restore collaborators to origin.'],
            },
          },
        },
      }),
    );
    expect(completed).toContain('Compaction pushed to origin.');
    expect(completed).toContain('Restore collaborators to origin.');
    expect(completed).toContain('--recover');
    expect(completed).not.toContain('Back Up and Rewrite History');
  });

  it('previews a deleted historical attachment without a current ticket label (HS2-JRT1ZP)', () => {
    const plan: CompactionPlan = {
      head: 'a'.repeat(40),
      branch: 'main',
      min_age_days: 90,
      oversized_bytes: 10_000_000,
      candidates: [
        {
          ticket_id: 'one',
          ticket_slug: '',
          attachment_id: 'att',
          filename: 'removed.png',
          path: 'attachments/one/att/removed.png',
          bytes: 2048,
          sha256: '',
          reason: 'orphaned',
          historical_blobs: [{ oid: 'b'.repeat(40), bytes: 2048 }],
          removed_in_commit: 'c'.repeat(40),
        },
      ],
      reclaimable_payload_bytes: 2048,
      fingerprint: 'd'.repeat(64),
    };
    const markup = String(
      RepositorySize({
        state: { status: 'ready', report: report([code, store]) },
        compaction: { status: 'ready', source: 'abc123', plan },
      }),
    );
    expect(markup).toContain('Removed attachment: removed.png (2 kB, orphaned)');
    expect(parseCompactionPlan(plan)).toBe(plan);
  });
});

describe('parseRepositorySizeReport (HS2-QF44T7)', () => {
  it('accepts the server shape and rejects an unknown level', () => {
    const value = report([code, store]);
    expect(parseRepositorySizeReport(value)).toBe(value);
    expect(() => parseRepositorySizeReport({ ...value, level: 'huge' })).toThrow(ServerResponseShapeError);
    expect(() => parseRepositorySizeReport({ ...value, entries: [{ ...code, git: { total_bytes: '1' } }] })).toThrow(
      ServerResponseShapeError,
    );
  });
});
