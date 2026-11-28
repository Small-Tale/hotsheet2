import { describe, expect, it } from 'vitest';

import { rightNowRows, type RightNowTargets, rightNowTiles } from '../stats-right-now';
import { GLANCE_FIXTURE_NOW as NOW, glanceScenario } from '../ux-demo/live-state-fixtures';
import { GlanceSparkline, glanceSparklinePoints } from './glance-sparkline';
import { StatsProjectsTable } from './stats-projects-table';
import { StatsRightNow, type StatsRightNowProps } from './stats-right-now';
import { GlobalWorkspaceSurface } from './workspace-composition-surfaces';

const targets: RightNowTargets = {
  projectFor: (state) => state.checkouts[0],
  terminalFor: (projectId) => `${projectId}:t1`,
};

function props(overrides: Partial<StatsRightNowProps> = {}): StatsRightNowProps {
  const states = glanceScenario();
  return {
    scope: { kind: 'all' },
    stores: states.length,
    tiles: rightNowTiles(states, NOW),
    rows: rightNowRows(states, NOW, targets),
    completed: { count: 9, sinceMs: Date.parse('2026-11-06T00:00:00Z') },
    asOfMs: NOW,
    ...overrides,
  };
}

const tile = (html: string, name: string) => new RegExp(`<[^>]*data-tile="${name}"[^>]*>`).exec(html)?.[0] ?? '';

describe('HS2-ERTNJ7 StatsRightNow', () => {
  it('renders above Ticket flow and usage in both Stats scopes', () => {
    for (const projectName of ['Hot Sheet 2', undefined]) {
      const html = String(GlobalWorkspaceSurface({ kind: 'stats', projectName, analytics: [], rightNow: props() }));
      const rightNow = html.indexOf('data-component="stats-right-now"');
      expect(rightNow).toBeGreaterThan(-1);
      expect(rightNow).toBeLessThan(html.indexOf('data-component="stats-dashboard"'));
    }
    expect(String(GlobalWorkspaceSurface({ kind: 'stats', analytics: [] }))).not.toContain('stats-right-now');
  });

  it('names its scope and the data-as-of freshness', () => {
    expect(String(StatsRightNow(props()))).toMatch(/All projects · live over the change stream · data as of \d/u);
    const project = String(StatsRightNow(props({ scope: { kind: 'project', name: 'Hot Sheet 2' } })));
    expect(project).toContain('Hot Sheet 2 · live over the change stream');
    expect(String(StatsRightNow(props({ asOfMs: undefined })))).toContain('waiting for live data');
  });

  it('shows every tile with live values; only Needs you tints, and only when non-zero', () => {
    const html = String(StatsRightNow(props()));
    expect(html).toContain('data-component="glance-gauge"');
    expect(html).toContain('Tickets updated · last 60 min');
    expect(html).toContain('data-component="glance-sparkline"');
    expect(html).toContain('Completed since');
    expect(html).toContain('3 / 4');
    expect(tile(html, 'needs-you')).toContain('data-attention="true"');
    expect(tile(html, 'needs-you')).toContain('warning');
    expect(tile(html, 'workers')).not.toContain('warning');
    const calm = String(StatsRightNow(props({ tiles: { ...props().tiles, needsYou: 0 } })));
    expect(tile(calm, 'needs-you')).toContain('data-attention="false"');
    expect(tile(calm, 'needs-you')).not.toContain('warning');
  });

  it('omits the completed tile until analytics load, and says when no CI run was reported (HS2-E16ZEK)', () => {
    const html = String(StatsRightNow(props({ completed: undefined })));
    expect(html).not.toContain('data-tile="completed"');
    expect(html).toContain('data-tile="ci"');
    expect(html).toContain('data-ci-status="unavailable"');
    expect(html).toContain('No CI reported');
    expect(html).not.toContain('data-ci-run-link');
  });

  it('shows the main CI run with its word, glyph, sha, age, and link, tinting only a failure (HS2-E16ZEK)', () => {
    const tile = (ci: StatsRightNowProps['tiles']['ci']) => {
      const html = String(StatsRightNow(props({ tiles: { ...props().tiles, ci } })));
      return /<[^>]*data-tile="ci"[\s\S]*?(?=<[^>]*data-right-now-projects)/u.exec(html)?.[0] ?? '';
    };
    const passed = tile({
      status: 'success',
      branch: 'main',
      shortSha: '1a2b3c4',
      ageMs: 12 * 60_000,
      runUrl: 'https://ci.example.test/runs/7',
    });
    expect(passed).toContain('Passed');
    expect(passed).toContain('data-lucide="circle-check"');
    expect(passed).toContain('main CI · 1a2b3c4 · 12m ago');
    expect(passed).toContain('href="https://ci.example.test/runs/7"');
    expect(passed).toContain('data-attention="false"');
    const failed = tile({ status: 'failure', branch: 'main', shortSha: '9f8e7d6', ageMs: 3 * 60_000 });
    expect(failed).toContain('Failed');
    expect(failed).toContain('data-lucide="circle-x"');
    expect(failed).toContain('data-attention="true"');
    expect(failed).not.toContain('data-ci-run-link');
    expect(tile({ status: 'in_progress', branch: 'main', ageMs: 4 * 60_000 })).toContain('main CI · running 4m');
    expect(tile({ status: 'queued', branch: 'release', ageMs: 60_000 })).toContain('release CI · waiting 1m');
    expect(tile({ status: 'cancelled', branch: 'main' })).toContain('data-lucide="circle-slash"');
    // HS2-D3JJ49: a transient run keeps its last known word; there is no Unknown word any more.
    const transient = tile({ status: 'in_progress', branch: 'main', ageMs: 60_000, transient: true });
    expect(transient).toContain('Running');
    expect(transient).not.toContain('Unknown');
    expect(transient).not.toContain('circle-help');
  });

  it('shows an empty state, not zero tiles, without live stores', () => {
    const html = String(StatsRightNow(props({ stores: 0, rows: [] })));
    expect(html).toContain('No Live Projects');
    expect(html).not.toContain('data-right-now-tiles');
  });
});

describe('HS2-ERTNJ7 StatsProjectsTable', () => {
  const html = String(StatsProjectsTable({ rows: rightNowRows(glanceScenario(), NOW, targets) }));
  const rowOf = (store: string) => new RegExp(`<tr[^>]*data-store="${store}"[\\s\\S]*?</tr>`).exec(html)?.[0] ?? '';

  it('lists rows in need order with state glyph and word', () => {
    const order = [...html.matchAll(/data-state="([a-z_]+)"/gu)].map((match) => match[1]);
    expect(order).toEqual(['may_be_stuck', 'waiting_on_you', 'active', 'stopped']);
    expect(rowOf('store-website')).toContain('data-lucide="hourglass"');
    expect(rowOf('store-website')).toContain('May be stuck');
    expect(rowOf('store-review')).toContain('data-lucide="hand"');
  });

  it('shows current work with phases, including final testing waiting on CI', () => {
    const active = rowOf('store-hotsheet');
    expect(active).toContain('HS2-R76MMW · initial testing');
    expect(active).toContain('HS2-TG2ZV3 · final testing (CI)');
    expect(rowOf('store-review')).toContain('2 requests waiting on you');
    expect(rowOf('store-kerf')).toContain('Nothing claimed');
  });

  it('shows last progress, ETA overrun, and tokens/min, with no Runway column (HS2-5A83MR)', () => {
    const stuck = rowOf('store-website');
    expect(stuck).toContain('34m ago');
    expect(stuck).toContain('+52m');
    expect(stuck).not.toContain('data-column="runway"');
    expect(stuck).not.toContain(' ready');
    expect(rowOf('store-hotsheet')).toContain('1.6k');
    expect(rowOf('store-review')).toMatch(/data-column="eta"[^>]*>—/u);
  });

  it('draws a 12-hour token sparkline per project with the current rate beside it (HS2-TRZGT3)', () => {
    const active = rowOf('store-hotsheet');
    expect(active).toContain('data-component="glance-sparkline"');
    expect(active).toContain(
      'aria-label="Hot Sheet 2 tokens per hour, last 12 hours: 20000, 34000, 42000, 51000, 60000, 72000, 66000, 70000, 81000, 88000, 90000, 95000"',
    );
    expect(active).toContain('1.6k/min');
    // A stopped project still shows the hours it worked earlier in the window.
    expect(rowOf('store-kerf')).toMatch(/data-component="glance-sparkline"[^>]*data-empty="false"/u);
  });

  it('routes the one unstick action per row through the existing delegated actions', () => {
    const stuck = rowOf('store-website');
    expect(stuck).toContain('data-action="open-terminal-project"');
    expect(stuck).toContain('data-terminal-key="website-c3d4:t1"');
    expect(stuck).toContain('Open Terminal');
    const waiting = rowOf('store-review');
    expect(waiting).toContain('data-action="open-linked-ticket"');
    expect(waiting).toContain('data-ticket-slug="UXR-2B7Q"');
    expect(waiting).toContain('data-ticket-project-id="review-e5f6"');
    expect(rowOf('store-hotsheet')).not.toContain('data-action=');
    expect(rowOf('store-kerf')).not.toContain('data-action=');
  });
});

describe('HS2-ERTNJ7 GlanceSparkline', () => {
  it('scales to its own maximum and names its values', () => {
    expect(glanceSparklinePoints([0, 2, 1])).toBe('0,26 60,2 120,14');
    expect(glanceSparklinePoints([0, 0])).toBe('0,26 120,26');
    expect(glanceSparklinePoints([])).toBe('');
    // HS2-RBFR94: a fixed scale (a percentage) keeps small values low instead of filling the box.
    expect(glanceSparklinePoints([0, 50], 100)).toBe('0,26 120,14');
    expect(glanceSparklinePoints([0, 150], 100)).toBe('0,26 120,2');
    const html = String(GlanceSparkline({ values: [1, 3], label: 'Updates', tone: 'success' }));
    expect(html).toContain('aria-label="Updates: 1, 3"');
    expect(html).toContain('data-tone="success"');
    expect(html).toContain('data-empty="false"');
    expect(String(GlanceSparkline({ values: [], label: 'None' }))).not.toContain('<polyline');
  });
});
