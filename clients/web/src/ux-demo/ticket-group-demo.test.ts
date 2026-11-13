import { beforeEach, describe, expect, it } from 'vitest';

import { resetTicketGroupDemo, TicketGroupDemo, TicketGroupSettings, ticketGroupSettings } from './ticket-group-demo';
import {
  closeTicketGroupDialogDemo,
  resetTicketGroupDialogDemo,
  setTicketGroupDialogScenario,
  TICKET_GROUP_DIALOG_SCENARIOS,
  TicketGroupDialogDemo,
  ticketGroupDialogScenarioName,
  TicketGroupDialogSettings,
  ticketGroupDialogSettings,
  ticketGroupDialogState,
} from './ticket-group-dialog-demo';

function fakeRoot() {
  const controls: Record<string, { value: string; checked: boolean }> = {};
  const root = {
    querySelector: (selector: string) => {
      const name = /\[name="([^"]+)"\]/.exec(selector)?.[1] ?? '';
      return (controls[name] ??= { value: 'stale', checked: true });
    },
  } as unknown as ParentNode;
  return { root, controls };
}

describe('TicketGroup demo (HS2-WZ3JGZ)', () => {
  beforeEach(() => {
    resetTicketGroupDemo(fakeRoot().root);
  });

  it('renders the production group in list and column presentations', () => {
    const markup = String(TicketGroupDemo());
    expect(markup.match(/data-component="ticket-group"/g)).toHaveLength(2);
    expect(markup).toContain('data-presentation="list"');
    expect(markup).toContain('data-presentation="column"');
    expect(markup).toContain('aria-label="Board drag &amp; drop, 3 tickets"');
    expect(markup).not.toContain('data-ticket-group-menu');
  });

  it('follows its settings and shows the group menu when one is open', () => {
    ticketGroupSettings.name.value = '  Search   syntax ';
    ticketGroupSettings.collapsed.value = true;
    ticketGroupSettings.dropTarget.value = true;
    ticketGroupSettings.menu.value = { x: 10, y: 20, group: 'Search syntax', scope: 'started' };
    const markup = String(TicketGroupDemo());
    // The group carries its fixture id and shows the typed display name (HS2-9KZSD4).
    expect(markup).toContain('data-ticket-group="7K3M9P2QXA"');
    expect(markup).toContain('>Search syntax<');
    expect(markup).not.toContain('merge-ticket-groups');
    ticketGroupSettings.duplicate.value = true;
    expect(String(TicketGroupDemo())).toContain('data-action="merge-ticket-groups"');
    expect(markup).not.toContain('data-ticket-slug=');
    expect(markup.match(/data-drop-target="true"/g)).toHaveLength(2);
    expect(markup).toContain('data-ticket-group-menu="true"');
    expect(markup).toContain('data-ticket-group-scope="started"');
    // An emptied name falls back to the fixture name instead of an unnamed group.
    ticketGroupSettings.name.value = '   ';
    expect(String(TicketGroupDemo())).toContain('>Board drag &amp; drop<');
  });

  it('offers Reset, which restores every setting and live control', () => {
    expect(String(TicketGroupSettings())).toContain('data-action="reset-settings"');
    ticketGroupSettings.collapsed.value = true;
    ticketGroupSettings.duplicate.value = true;
    ticketGroupSettings.menu.value = { x: 1, y: 1, group: 'x', scope: 'list' };
    const { root, controls } = fakeRoot();
    resetTicketGroupDemo(root);
    expect(ticketGroupSettings.collapsed.value).toBe(false);
    expect(ticketGroupSettings.menu.value).toBeUndefined();
    expect(controls['group-name']).toMatchObject({ value: 'Board drag & drop' });
    expect(controls['group-collapsed']).toMatchObject({ checked: false });
    expect(controls['group-drop-target']).toMatchObject({ checked: false });
    expect(controls['group-duplicate']).toMatchObject({ checked: false });
    expect(ticketGroupSettings.duplicate.value).toBe(false);
  });
});

describe('TicketGroupDialog demo (HS2-TX17CM, HS2-8CDFKR)', () => {
  beforeEach(() => {
    resetTicketGroupDialogDemo(fakeRoot().root);
  });

  it('maps every scenario to the production dialog state it previews', () => {
    expect(TICKET_GROUP_DIALOG_SCENARIOS.map(({ value }) => value)).toEqual([
      'new',
      'duplicate',
      'rename',
      'rename-busy',
      'rename-error',
    ]);
    expect(ticketGroupDialogState('new', '', 1)).toMatchObject({ mode: 'new', name: '' });
    expect(ticketGroupDialogScenarioName('duplicate')).toBe('board drag & drop');
    expect(ticketGroupDialogState('rename', 'x', 1)).toMatchObject({
      mode: 'rename',
      group: '7K3M9P2QXA',
      groups: ['Search syntax'],
    });
    expect(ticketGroupDialogState('rename-busy', 'x', 1).busy).toBe(true);
    expect(ticketGroupDialogState('rename-error', 'x', 1).error).toContain('locked');
  });

  it('opens each scenario with its own name and a fresh field session', () => {
    const before = ticketGroupDialogSettings.session.value;
    setTicketGroupDialogScenario('duplicate');
    expect(ticketGroupDialogSettings.session.value).toBe(before + 1);
    const markup = String(TicketGroupDialogDemo());
    expect(markup).toContain('already exists');
    expect(markup).toContain('Create Group');
    setTicketGroupDialogScenario('rename');
    expect(String(TicketGroupDialogDemo())).toContain('Rename Group');
  });

  it('closes with a reported event, offers Open Dialog, and resets', () => {
    closeTicketGroupDialogDemo('New group “Polish” requested.');
    const closed = String(TicketGroupDialogDemo());
    expect(closed).toContain('Open Dialog');
    expect(closed).toContain('New group “Polish” requested.');
    expect(closed).not.toContain('data-dialog-component="ticket-group-dialog"');
    expect(String(TicketGroupDialogSettings())).toContain('data-action="reset-settings"');
    setTicketGroupDialogScenario('rename-error');
    const { root, controls } = fakeRoot();
    resetTicketGroupDialogDemo(root);
    expect(ticketGroupDialogSettings.scenario.value).toBe('new');
    expect(ticketGroupDialogSettings.open.value).toBe(true);
    expect(controls['group-dialog-scenario']).toMatchObject({ value: 'new' });
  });
});
