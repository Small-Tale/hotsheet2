import { describe, expect, it } from 'vitest';

import type { RepositorySizeEntry, RepositorySizeLevel, RepositorySizeReport } from '../api';
import { RepositorySizeBanner, showsRepositorySizeBanner } from './repository-size-banner';

const THRESHOLDS = { warning_bytes: 5_000_000_000, critical_bytes: 10_000_000_000 };

function entry(label: string, level: RepositorySizeLevel, kind: RepositorySizeEntry['kind'] = 'ticket_store') {
  return {
    kind,
    label,
    locator: `/work/${label}`,
    status: 'measured',
    shared_with_code: false,
    level,
  } satisfies RepositorySizeEntry;
}

function report(level: RepositorySizeLevel, entries: RepositorySizeEntry[]): RepositorySizeReport {
  return { checkout_id: 'web-1a2b3c', thresholds: THRESHOLDS, level, entries };
}

const WARNING = report('warning', [entry('web', 'ok', 'code'), entry('web.hs2', 'warning')]);
const CRITICAL = report('critical', [entry('web', 'critical', 'code'), entry('web.hs2', 'critical')]);

describe('showsRepositorySizeBanner (HS2-P1GYWH)', () => {
  it('shows only for a loaded report over a limit that was not dismissed at that level or higher', () => {
    expect(showsRepositorySizeBanner(undefined, undefined)).toBe(false);
    expect(showsRepositorySizeBanner(report('ok', [entry('web', 'ok', 'code')]), undefined)).toBe(false);
    expect(showsRepositorySizeBanner(WARNING, undefined)).toBe(true);
    expect(showsRepositorySizeBanner(WARNING, 'warning')).toBe(false);
    expect(showsRepositorySizeBanner(WARNING, 'critical')).toBe(false);
    // A dismissed warning comes back when the repository crosses the hard limit.
    expect(showsRepositorySizeBanner(CRITICAL, 'warning')).toBe(true);
    expect(showsRepositorySizeBanner(CRITICAL, 'critical')).toBe(false);
  });
});

describe('RepositorySizeBanner (HS2-P1GYWH)', () => {
  it('names the repository over the soft limit and links to the statistics view', () => {
    const html = String(RepositorySizeBanner({ report: WARNING, projectId: 'web-1a2b3c' }));
    expect(html).toContain('data-component="repository-size-banner"');
    expect(html).toContain('data-tone="warning"');
    expect(html).toContain("Ticket store: web.hs2 is over GitHub's 5 GB soft limit");
    expect(html).toContain('GitHub rejects repositories over 10 GB');
    expect(html).toContain('data-action="open-project-stats"');
    expect(html).toContain('data-project-id="web-1a2b3c"');
    expect(html).toContain('data-action="dismiss-repository-size-warning"');
    expect(html).toContain('data-level="warning"');
    expect(html).toContain('triangle-alert');
  });

  it('names every repository over the hard limit with the push-rejection detail', () => {
    const html = String(RepositorySizeBanner({ report: CRITICAL, dismissed: 'warning', projectId: 'web-1a2b3c' }));
    expect(html).toContain("Code repository: web and Ticket store: web.hs2 are over GitHub's 10 GB hard limit");
    expect(html).toContain('GitHub rejects pushes to repositories this large');
    expect(html).toContain('data-level="critical"');
    expect(html).toContain('octagon-alert');
  });

  it('renders nothing within limits or once dismissed', () => {
    expect(RepositorySizeBanner({ report: undefined, projectId: 'p' })).toBeUndefined();
    expect(RepositorySizeBanner({ report: WARNING, dismissed: 'warning', projectId: 'p' })).toBeUndefined();
  });
});
