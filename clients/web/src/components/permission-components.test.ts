import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it, vi } from 'vitest';

import {
  type PermissionHistoryItem,
  type PermissionItem,
  type PermissionRule,
  permissionRuleKey,
} from '../permission-notifications';
import { TOP_LAYER_DISMISS_ATTRIBUTE, TOP_LAYER_OVERLAY_ATTRIBUTE } from '../top-layer-overlay';
import { NotificationCenter } from './notification-center';
import { NotificationNavigation } from './notification-navigation';
import { NotificationsPausedBanner } from './notifications-paused-banner';
import {
  PermissionRequestCard,
  PermissionRequestPopup,
  updatePermissionCountdownText,
} from './permission-request-card';
import { PERMISSION_RULE_BATCH_SIZE, permissionRulesList, PermissionRulesSettings } from './permission-rules-settings';

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
    // The project pill is a truncating Kerf Badge with its name as the tooltip (HS2-B0B28R).
    expect(markup).toMatch(
      /class="notification-card__project"><span[^>]*data-component="badge"[^>]*data-truncate[^>]*title="Hot Sheet 2"|class="notification-card__project"><span[^>]*title="Hot Sheet 2"[^>]*data-truncate/,
    );
    expect(markup).toContain('npm run test\nnpm run lint');
    expect(markup).toContain('Ignore');
    expect(markup).toContain('Deny');
    expect(markup).toContain('Always Allow');
    expect(markup).toContain('Allow Once');
    expect(markup).toContain('data-scope="always"');
    expect(markup).toContain('Auto-allow in');
    expect(markup).toContain('aria-label="Stop auto-allow Countdown"');
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
    expect(markup).not.toContain('notification-card__details');
  });

  it('labels automatic denial explicitly', () => {
    const markup = String(PermissionRequestCard({ item: pending, countdown: '0:13', countdownAction: 'deny' }));
    expect(markup).toContain('Auto-deny in');
    expect(markup).toContain('aria-label="Stop auto-deny Countdown"');
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
    expect(markup).toContain('aria-label="Permission Request"');
    expect(markup).toContain('>Allow</span></button>');
    expect(markup).not.toContain('Always Allow');
  });

  it('lifts only the shell popup into the top layer as a manual popover (HS2-ZESCM2)', () => {
    const inline = String(PermissionRequestPopup({ item: pending })),
      top = String(PermissionRequestPopup({ item: pending, layer: 'top' }));
    expect(inline).toContain('data-layer="inline"');
    expect(inline).not.toContain('popover=');
    expect(inline).not.toContain(TOP_LAYER_OVERLAY_ATTRIBUTE);
    expect(top).toContain('data-layer="top"');
    expect(top).toContain('data-elevation="popup-contained"');
    expect(inline).toContain('data-elevation="popup"');
    expect(top).toContain('popover="manual"');
    // Both attributes the shared watcher (`openTopLayerOverlays`) selects on, so it calls showPopover().
    expect(top).toContain(TOP_LAYER_OVERLAY_ATTRIBUTE);
    expect(top).toContain('Allow Once');
    // A `<dialog>`, so the watcher can lift it with showModal() above an open modal dialog, focusing the
    // popup itself rather than a decision button (HS2-MAE27T); the other layers stay `<aside>`.
    expect(top).toMatch(/^<dialog [^>]*tabindex="-1"/);
    expect(inline).toMatch(/^<aside /);
    // Escape activates the popup's client-only Ignore while it is lifted above a modal (HS2-S8K9BG).
    expect(top).toMatch(new RegExp(`<button [^>]*${TOP_LAYER_DISMISS_ATTRIBUTE}[^>]*>[\\s\\S]*?Ignore`));
    // The in-flow layer is how the AI Conversation's foreground places its copy without restyling it (HS2-M2W2DP).
    const flow = String(PermissionRequestPopup({ item: pending, layer: 'flow' }));
    expect(flow).toContain('data-layer="flow"');
    expect(flow).not.toContain('popover=');
    const css = readFileSync(resolve(import.meta.dirname, 'permission-request-card.css'), 'utf8');
    expect(css).toMatchSource(
      /\.permission-request-popup\[data-layer='flow'\] \{[^}]*position:relative;[^}]*inset:auto;[^}]*width:min\(remify\(704px\), 100%\);[^}]*pointer-events:auto/,
    );
    // The phone placement is for the corner-anchored layers only, so it never offsets the in-flow copy
    // inside its host (HS2-SH3DR7).
    expect(css).toContain(
      "  .permission-request-popup[data-layer='inline'],\n  .permission-request-popup[data-layer='top'] {",
    );
    expect(css).toContain('@supports (position-anchor: --app-shell-main)');
    expect(css).toMatchSource(
      /\.permission-request-popup\[data-layer='top'\] \{[^}]*max-width:none;[^}]*border-radius:remify\(16px\);[^}]*box-shadow:var\(--wa-shadow-l\);[^}]*transition:none;/,
    );
    expect(readFileSync(resolve(import.meta.dirname, 'notification-card.css'), 'utf8')).toMatchSource(
      /\.notification-card\[data-elevation='popup-contained'\] \{[^}]*border-color:var\(--wa-color-neutral-border-loud\);[^}]*box-shadow:none;/,
    );
    expect(css).toMatchSource(/\.permission-request-popup\[data-layer='top'\]\[open\] \{\s*display:block;/);
    expect(css).toMatchSource(/\.permission-request-popup\[data-layer='top'\]::backdrop \{\s*background:transparent;/);
    expect(css).toContain('position-anchor: --app-shell-main;');
    expect(readFileSync(resolve(import.meta.dirname, 'app-shell.css'), 'utf8')).toContain(
      'anchor-name: --app-shell-main;',
    );
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
    const resolvedWithoutAction: PermissionHistoryItem = { ...history, tool: 'ToolSearch', action: '' };
    const markup = String(PermissionRequestCard({ item: resolvedWithoutAction }));
    const css = readFileSync(resolve(import.meta.dirname, 'notification-card.css'), 'utf8');
    expect(markup).toContain('data-resolved="true"');
    expect(markup).toContain('data-elevation="flat"');
    expect(markup).toContain('ToolSearch: Decision made outside Hot Sheet');
    expect(markup).not.toContain('notification-card__details');
    expect(markup).not.toContain('notification-card__footer');
    // The shared NotificationCard keeps the footer's bottom inset on a card without a footer (HS2-XS71FB).
    expect(css).toMatchSource(
      /\.notification-card > :last-child:not\(\.notification-card__footer\)[^{]*\{[^}]*margin-bottom: remify\(16px\)/,
    );
  });

  it('uses the same canonical card gap within and between pending/history groups (HS2-D38KZF)', () => {
    const markup = String(NotificationCenter({ pending: [pending], history: [history] }));
    // Preserve the established 12px notification rhythm within and between groups.
    expect(markup.match(/data-component="list"/g)).toHaveLength(3);
    expect(markup.indexOf('data-state="pending"')).toBeLessThan(markup.indexOf('data-state="external"'));
  });

  it('renders explicit empty states', () => {
    const markup = String(NotificationCenter({ pending: [], history: [], title: 'Pending' }));
    expect(markup).toContain('No requests need your attention.');
    expect(String(NotificationCenter({ pending: [], history: [], title: 'Last 7 Days' }))).toContain(
      'No notification history in last 7 days.',
    );
  });

  it('pads as a page by default and drops its inset where a host already insets it (HS2-8FS5BJ)', () => {
    expect(String(NotificationCenter({ pending: [], history: [] }))).toContain(
      'class="notification-center" data-component="notification-center" data-inset="page"',
    );
    expect(String(NotificationCenter({ pending: [], history: [], inset: 'flush' }))).toContain(
      'class="notification-center" data-component="notification-center" data-inset="flush"',
    );
    expect(String(NotificationCenter({ pending: [], history: [] }))).toContain('data-control-insets="trbl"');
    expect(String(NotificationCenter({ pending: [], history: [], inset: 'flush' }))).not.toContain(
      'data-control-insets',
    );
    // The ticket rail asks for the flush inset instead of restyling the center from its stylesheet.
    expect(readFileSync(resolve(import.meta.dirname, 'terminal-ticket-rail.css'), 'utf8')).not.toContain(
      'notification-center',
    );
  });

  it('offers the three notification views with counts and current state', () => {
    const markup = String(
      NotificationNavigation({ selected: 'day', counts: { pending: 2, day: 3, week: 5 }, collapseControl: true }),
    );
    expect(markup).toContain('data-component="pane"');
    expect(markup).toContain('class="kui-text" data-component="text"');
    expect(markup).toContain('data-font="default" data-border="none">Notifications</h2>');
    // The Pane's scrolling content is the navigation landmark (HS2-RWGQWN).
    expect(markup).toContain('<nav class="kui-pane__content kui-content" aria-label="Notification views">');
    expect(markup).toContain('data-component="list"');
    expect(markup).not.toContain('divider-sides');
    for (const label of ['Pending', 'Last 24 Hours', 'Last 7 Days']) expect(markup).toContain(label);
    expect(markup.match(/notification-navigation__count/g)).toHaveLength(3);
    // Pending notifications take the danger Badge tone, matching ViewNavigation's attention counts.
    expect(markup).toMatch(/notification-navigation__count" data-component="badge" data-tone="danger"[^>]*>2<\/span>/);
    expect(markup).toContain('data-item-id="day"');
    expect(markup).toContain('aria-current="page"');
    expect(String(NotificationNavigation({ selected: 'pending', counts: { pending: 0, day: 0, week: 0 } }))).toMatch(
      /data-tone="neutral"[^>]*>0<\/span>/,
    );
  });
});

describe('recent remembered answers (HS2-16XPHH)', () => {
  const rules: PermissionRule[] = Array.from({ length: 12 }, (_, index) => ({
    project: '/p',
    tool: index % 2 ? 'WebFetch' : 'Bash',
    action: `hmac-sha256:v2:${'a'.repeat(16)}:${'0'.repeat(56)}${index.toString().padStart(8, '0')}`,
    decision: index % 2 ? 'deny' : 'allow',
    scope: index % 2 ? 'session' : 'always',
  }));

  it('shows the newest three and a route to all answers', () => {
    const markup = String(PermissionRulesSettings({ rules }));
    expect(markup).toMatch(/data-component="list-header"[^]*<h3[^>]*>[^]*Remembered Answers/u);
    expect(markup.match(/data-permission-rule-scope=/g)).toHaveLength(3);
    expect(markup.match(/data-action="change-permission-rule-scope"/g)).toHaveLength(3);
    expect(markup).toContain('data-scope="session"');
    expect(markup).toContain('data-scope="always"');
    expect(markup).toContain('WebFetch request · 00000011');
    expect(markup).toContain('Show all 12 answers');
    expect(markup).not.toContain('00000008');
  });

  it('marks denied, selected, and busy answers in both row compositions', () => {
    const selectedKey = permissionRuleKey(rules[11]);
    const preview = String(PermissionRulesSettings({ rules }));
    expect(preview).toContain('data-permission-rule-decision="deny"');
    const dialog = String(
      PermissionRulesSettings({
        rules,
        dialog: { query: '', visibleCount: PERMISSION_RULE_BATCH_SIZE, selected: [selectedKey], busy: true },
      }),
    );
    expect(dialog).toContain('data-rule-selected="true"');
    expect(dialog).toContain('data-rule-disabled="true"');
  });

  it('uses the same ListItem and leading shield slot in preview and dialog (HS2-1Q8EF7)', () => {
    const preview = String(PermissionRulesSettings({ rules })),
      dialog = String(PermissionRulesSettings({ rules, dialog: { query: '', visibleCount: 100 } }));
    expect(preview.match(/class="permission-rules-settings__answer"><button[^>]*class="kui-list-item"/g)).toHaveLength(
      3,
    );
    expect(dialog.match(/class="permission-rules-settings__answer"><button[^>]*class="kui-list-item"/g)).toHaveLength(
      15,
    );
    expect(preview).toMatch(/class="kui-list-item__icon"><svg data-lucide="shield-/u);
    expect(dialog).toMatch(
      /data-action="toggle-permission-rule-row"[^]*class="kui-list-item__icon"><svg data-lucide="shield-/u,
    );
  });

  it('filters safe labels and moves through 100-row batches across shrink and refill', () => {
    const many = Array.from({ length: 205 }, (_, index) => ({
      ...rules[index % rules.length],
      action: `hmac-sha256:v2:${'a'.repeat(16)}:${'0'.repeat(56)}${index.toString().padStart(8, '0')}`,
    }));
    const first = permissionRulesList(many, '', 100);
    expect(first.rows).toHaveLength(100);
    expect(first.remainingCount).toBe(105);
    expect(permissionRulesList(many, '', 200).rows).toHaveLength(200);
    expect(permissionRulesList(many, '', 300).rows).toHaveLength(205);
    expect(permissionRulesList(many.slice(0, 60), '', 300).visibleCount).toBe(100);
    expect(permissionRulesList([], 'Bash', 300).rows).toEqual([]);
    expect(permissionRulesList(many, '', 100).rows).toHaveLength(100);
    expect(permissionRulesList(rules, '00000010', 100).rows).toHaveLength(1);
    expect(permissionRulesList([{ ...rules[0], action: 'secret-shell-command' }], 'secret', 100).rows).toEqual([]);
  });

  it('shows a scrollable list and accessible placeholder search (HS2-988X4X, HS2-4HTTMK)', () => {
    const markup = String(PermissionRulesSettings({ rules, dialog: { query: 'deny', visibleCount: 100 } }));
    expect(markup).toContain('permission-rules-settings__all');
    expect(markup).not.toContain('Previous');
    expect(markup).not.toContain('Next');
    expect(markup).not.toContain('matching answers · newest first');
    expect(markup).toContain('All Remembered Answers');
    expect(markup).toContain('aria-label="Search Remembered Answers"');
    expect(markup).toContain('placeholder="Filter"');
    expect(markup).not.toContain('kui-text-field__label');
  });
});

describe('notification pause controls (HS2-QYA9SC)', () => {
  it('offers Pause notifications in the popup only, beside Ignore', () => {
    vi.spyOn(Date, 'now').mockReturnValue(10);
    const popup = String(PermissionRequestPopup({ item: pending, layer: 'top' }));
    expect(popup).toContain('data-action="pause-notifications"');
    expect(popup.indexOf('data-action="ignore-permission"')).toBeLessThan(
      popup.indexOf('data-action="pause-notifications"'),
    );
    const listed = String(PermissionRequestCard({ item: pending, presentation: 'list' }));
    expect(listed).toContain('data-action="ignore-permission"');
    expect(listed).not.toContain('pause-notifications');
  });

  it('toggles the sidebar footer between Pause and Resume', () => {
    const counts = { pending: 2, day: 1, week: 4 };
    const active = String(NotificationNavigation({ selected: 'pending', counts }));
    expect(active).toContain('data-action="pause-notifications"');
    expect(active).toContain('Pause Notifications');
    expect(active).toContain('data-lucide="bell-off"');
    const paused = String(NotificationNavigation({ selected: 'pending', counts, paused: true }));
    expect(paused).toContain('data-action="resume-notifications"');
    expect(paused).toContain('Resume Notifications');
    expect(paused).not.toContain('pause-notifications');
  });

  it('banners the pause with the waiting count and a Resume action', () => {
    const banner = String(NotificationsPausedBanner({ waiting: 2 }));
    expect(banner).toContain('Notifications paused');
    expect(banner).toContain('2 permission requests waiting.');
    expect(banner).toContain('data-action="resume-notifications"');
    expect(banner).toContain('class="shell-banner" data-component="notifications-paused-banner" data-tone="info"');
    expect(banner).toContain('data-lucide="bell-off"');
  });
});

describe('chat-scoped permission answers (HS2-JNHAYB)', () => {
  it('offers Allow for This Chat and announces a group of identical requests', () => {
    const single = String(PermissionRequestCard({ item: pending, presentation: 'popup' }));
    expect(single).toMatch(/data-scope="chat"[^>]*>[^]*Allow for This Chat/);
    expect(single).not.toContain('data-permission-group-count');
    const grouped = String(PermissionRequestCard({ item: pending, presentation: 'popup', groupCount: 3 }));
    expect(grouped).toContain('data-permission-group-count="3"');
    expect(grouped).toContain('3 identical requests are waiting');
    const remembered = String(
      PermissionRequestCard({ item: { ...pending, decision: 'allow', scope: 'chat', resolvedAt: 20 } }),
    );
    expect(remembered).toContain('allowed for this chat');
    expect(remembered).not.toContain('Allow for This Chat');
  });
  it('lists remembered rules with their scope and a revoke action, or an empty state', () => {
    const empty = String(PermissionRulesSettings({ rules: [] }));
    expect(empty).toContain('No Remembered Answers');
    const first = `hmac-sha256:v2:${'a'.repeat(16)}:${'a'.repeat(56)}12345678`,
      second = `hmac-sha256:v2:${'b'.repeat(16)}:${'b'.repeat(56)}87654321`;
    const markup = String(
      PermissionRulesSettings({
        rules: [
          {
            project: '/p',
            connection: 'chat-a',
            tool: 'Bash',
            action: first,
            decision: 'allow',
            scope: 'chat',
          },
          { project: '/p', tool: 'Bash', action: second, decision: 'deny', scope: 'always' },
        ],
        revoking: [JSON.stringify(['/p', '', 'Bash', second])],
        message: 'Could not revoke the rule.',
      }),
    );
    expect(markup).toContain('Bash request · 12345678');
    expect(markup).toContain('data-truncate="middle"');
    expect(markup).toContain('kui-text__truncate-tail">12345678');
    expect(markup).toContain('This chat only, until it closes');
    expect(markup).toContain('Denied · Persistent');
    expect(markup).not.toContain('Details hidden');
    expect(markup).not.toContain('This project, always');
    expect(markup).not.toContain('npm test');
    expect(markup).not.toContain('rm -rf build');
    expect(markup.match(/data-action="revoke-permission-rule"/g)).toHaveLength(2);
    expect(markup.match(/data-action="change-permission-rule-scope"/g)).toHaveLength(1);
    expect(markup).toContain('aria-label="Set Bash request · 87654321 to Session"');
    expect(markup).toContain('Set Session');
    expect(markup).toMatch(
      /aria-label="Revoke Bash request · 87654321"[^>]*disabled|disabled[^>]*aria-label="Revoke Bash request · 87654321"/,
    );
    expect(markup).toContain('Could not revoke the rule.');
  });
});
