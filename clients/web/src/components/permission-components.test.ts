import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it, vi } from 'vitest';

import type { PermissionHistoryItem, PermissionItem } from '../permission-notifications';
import { NotificationCenter } from './notification-center';
import { PermissionRequestCard, PermissionRequestPopup, updatePermissionCountdownText } from './permission-request-card';

const pending: PermissionItem = { id: 7, connection: 'claude-main', tool: 'Bash', action: 'npm run test\nnpm run lint', always_allow_supported: true, key: 'project:7', projectId: 'project', projectName: 'Hot Sheet 2', agent: 'Claude', role: 'main worker', receivedAt: 10, ignored: false };
const history: PermissionHistoryItem = { ...pending, decision: 'external', resolvedAt: 20 };

describe('permission presentation components', () => {
  it('renders the complete always-allow decision set and scoped action contract', () => {
    vi.spyOn(Date, 'now').mockReturnValue(10);
    const markup = String(PermissionRequestCard({ item: pending, countdown: '0:13', explanation: 'The project test suite needs a local process.' }));
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
    expect(readFileSync(resolve(import.meta.dirname, 'permission-request-card.css'), 'utf8')).toContain('.permission-request-card__timer strong { margin-left: .3em; }');
  });

  it('uses only the first action line as an edit target', () => {
    const markup = String(PermissionRequestCard({ item: { ...pending, tool: 'Edit', action: '/tmp/file.ts\nA detailed patch summary' } }));
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

  it('groups pending and newest-first history without hiding external decisions', () => {
    const allowed: PermissionHistoryItem = { ...pending, key: 'project:8', id: 8, decision: 'allow', scope: 'once', resolvedAt: 30 };
    const markup = String(NotificationCenter({ pending: [pending], history: [allowed, history], countdowns: { 'project:7': '1:00' } }));
    expect(markup).toContain('1 pending notification');
    expect(markup).toContain('Pending');
    expect(markup).toContain('Previous');
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
    expect(css).toMatch(/permission-request-card--list\[data-resolved="true"\][^{]*\{[^}]*padding-bottom: 1rem/);
  });

  it('renders explicit empty states', () => {
    const markup = String(NotificationCenter({ pending: [], history: [] }));
    expect(markup).toContain('No requests need your attention.');
    expect(markup).toContain('Previous decisions will appear here.');
  });
});
