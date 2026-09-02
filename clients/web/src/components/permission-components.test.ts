import { describe, expect, it, vi } from 'vitest';

import type { PermissionHistoryItem, PermissionItem } from '../permission-notifications';
import { NotificationCenter } from './notification-center';
import { PermissionRequestCard, PermissionRequestPopup } from './permission-request-card';

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
    expect(markup).toContain('Stop auto-allow');
    expect(markup).toContain('title="Stop automatic allow for this request"');
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
    expect(markup).toContain('Stop auto-deny');
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

  it('renders explicit empty states', () => {
    const markup = String(NotificationCenter({ pending: [], history: [] }));
    expect(markup).toContain('No requests need your attention.');
    expect(markup).toContain('Previous decisions will appear here.');
  });
});
