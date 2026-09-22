import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it, vi } from 'vitest';

import type { PermissionHistoryItem, PermissionItem } from '../permission-notifications';
import { NotificationCenter } from './notification-center';
import { NotificationNavigation } from './notification-navigation';
import {
  PermissionRequestCard,
  PermissionRequestPopup,
  updatePermissionCountdownText,
} from './permission-request-card';

const pending: PermissionItem = {
  id: 7,
  connection: 'claude-main',
  tool: 'Bash',
  action: 'npm run test\nnpm run lint',
  always_allow_supported: true,
  key: 'project:7',
  projectId: 'project',
  projectName: 'Hot Sheet 2',
  agent: 'Claude',
  role: 'main worker',
  receivedAt: 10,
  ignored: false,
};
const history: PermissionHistoryItem = { ...pending, decision: 'external', resolvedAt: 20 };

describe('permission presentation components', () => {
  it('renders the complete always-allow decision set and scoped action contract', () => {
    vi.spyOn(Date, 'now').mockReturnValue(10);
    const markup = String(
      PermissionRequestCard({
        item: pending,
        countdown: '0:13',
        explanation: 'The project test suite needs a local process.',
      }),
    );
    expect(markup).toContain('Wants permission to run a command');
    expect(markup).toContain('Hot Sheet 2');
    expect(markup).toContain('npm run test\nnpm run lint');
    expect(markup).toContain('Ignore');
    expect(markup).toContain('Deny');
    expect(markup).toContain('Always Allow');
    expect(markup).toContain('Allow Once');
    expect(markup).toContain('data-scope="always"');
    expect(markup).toContain('Auto-allow in');
    expect(markup).toContain('aria-label="Stop auto-allow countdown"');
    expect(markup).toContain('title="Stop auto-allow countdown for this request"');
    expect(markup).toContain('data-lucide="pause"');
    expect(readFileSync(resolve(import.meta.dirname, 'permission-request-card.css'), 'utf8')).toContainSource(
      '.permission-request-card__timer strong { margin-left: .3em; }',
    );
  });

  it('uses only the first action line as an edit target', () => {
    const markup = String(
      PermissionRequestCard({ item: { ...pending, tool: 'Edit', action: '/tmp/file.ts\nA detailed patch summary' } }),
    );
    expect(markup).toContain('Wants permission to edit /tmp/file.ts</strong>');
    expect(markup).not.toContain('edit /tmp/file.ts\nA detailed');
  });

  it('omits the details box when the permission action is empty', () => {
    const markup = String(PermissionRequestCard({ item: { ...pending, tool: 'ToolSearch', action: '  \n ' } }));
    expect(markup).toContain('Wants permission to use ToolSearch');
    expect(markup).not.toContain('permission-request-card__details');
  });

  it('labels automatic denial explicitly', () => {
    const markup = String(PermissionRequestCard({ item: pending, countdown: '0:13', countdownAction: 'deny' }));
    expect(markup).toContain('Auto-deny in');
    expect(markup).toContain('aria-label="Stop auto-deny countdown"');
  });

  it('updates only the matching countdown text and skips unchanged or absent output', () => {
    const output = { textContent: '0:13' };
    const matching = { dataset: { permissionCountdownKey: 'project:7' }, querySelector: () => output };
    const other = { dataset: { permissionCountdownKey: 'project:8' }, querySelector: () => ({ textContent: '1:00' }) };
    const root = { querySelectorAll: () => [other, matching] } as unknown as ParentNode;
    expect(updatePermissionCountdownText(root, 'project:7', '0:12')).toBe(true);
    expect(output.textContent).toBe('0:12');
    expect(updatePermissionCountdownText(root, 'project:7', '0:12')).toBe(false);
    expect(updatePermissionCountdownText(root, 'missing', '0:10')).toBe(false);
  });

  it('removes always scope when unsupported and exposes popup semantics', () => {
    const markup = String(PermissionRequestPopup({ item: { ...pending, always_allow_supported: false } }));
    expect(markup).toContain('aria-label="Permission request"');
    expect(markup).toContain('>Allow</button>');
    expect(markup).not.toContain('Always Allow');
  });

  it('restores a failed decision as an actionable popup with its communication error', () => {
    const markup = String(
      PermissionRequestPopup({
        item: pending,
        state: 'failed',
        error: 'Could not send the permission decision. Bridge unavailable.',
      }),
    );
    expect(markup).toContain('data-state="failed"');
    expect(markup).toContain('role="alert"');
    expect(markup).toContain('Could not send the permission decision. Bridge unavailable.');
    expect(markup).not.toContain(
      'data-action="resolve-permission" data-decision="deny" data-scope="once" data-request-key="project:7" disabled',
    );
  });

  it('renders the selected notification slice without hiding external decisions', () => {
    const allowed: PermissionHistoryItem = {
      ...pending,
      key: 'project:8',
      id: 8,
      decision: 'allow',
      scope: 'once',
      resolvedAt: 30,
    };
    const markup = String(
      NotificationCenter({ pending: [pending], history: [allowed, history], countdowns: { 'project:7': '1:00' } }),
    );
    expect(markup).toContain('allowed permission');
    expect(markup).toContain('Decision made outside Hot Sheet');
    expect(markup).toContain('1:00');
  });

  it('marks responded list items for footer-equivalent bottom padding even without details or actions', () => {
    const resolvedWithoutAction: PermissionHistoryItem = { ...history, action: '' };
    const markup = String(PermissionRequestCard({ item: resolvedWithoutAction }));
    const css = readFileSync(resolve(import.meta.dirname, 'permission-request-card.css'), 'utf8');
    expect(markup).toContain('data-resolved="true"');
    expect(markup).not.toContain('permission-request-card__details');
    expect(markup).not.toContain('permission-request-card__footer');
    expect(css).toMatchSource(
      /permission-request-card--list\[data-resolved="true"\][^{]*\{[^}]*padding-bottom: remify\(16px\)/,
    );
  });

  it('uses the same canonical card gap within and between pending/history groups (HS2-D38KZF)', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'notification-center.css'), 'utf8');
    expect(css).toMatch(/\.notification-center \{[^}]*display: grid;[^}]*gap: var\(--kui-space-s\);/);
    expect(css).toMatch(/\.notification-center__items \{[^}]*display: grid;[^}]*gap: var\(--kui-space-s\);/);
    expect(css).not.toContain('notification-center__section');
    expect(css).not.toContain('notification-center__header');
    const markup = String(NotificationCenter({ pending: [pending], history: [history] }));
    expect(markup.match(/class="notification-center__items"/g)).toHaveLength(2);
    expect(markup.indexOf('data-state="pending"')).toBeLessThan(markup.indexOf('data-state="external"'));
  });

  it('renders explicit empty states', () => {
    const markup = String(NotificationCenter({ pending: [], history: [], title: 'Pending' }));
    expect(markup).toContain('No requests need your attention.');
    expect(String(NotificationCenter({ pending: [], history: [], title: 'Last 7 Days' }))).toContain(
      'No notification history in last 7 days.',
    );
  });

  it('offers the three notification views with counts and current state', () => {
    const markup = String(
      NotificationNavigation({ selected: 'day', counts: { pending: 2, day: 3, week: 5 }, collapseControl: true }),
    );
    expect(markup).toContain('data-component="pane"');
    expect(markup).toContain('class="kui-list-header__label">Notifications</h2>');
    expect(markup).toContain('<nav aria-label="Notification views">');
    expect(markup).toContain('data-component="list"');
    expect(markup).not.toContain('divider-sides');
    for (const label of ['Pending', 'Last 24 Hours', 'Last 7 Days']) expect(markup).toContain(label);
    expect(markup.match(/class="kui-list-item__count"/g)).toHaveLength(3);
    expect(markup).toContain('class="kui-list-item__count" data-attention="true">2</small>');
    expect(markup).toContain('data-item-id="day"');
    expect(markup).toContain('aria-current="page"');
    expect(String(NotificationNavigation({ selected: 'pending', counts: { pending: 0, day: 0, week: 0 } }))).toContain(
      'class="kui-list-item__count" data-attention="false">0</small>',
    );
  });
});
