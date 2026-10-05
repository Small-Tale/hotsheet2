import { uiColor, type UiColorName } from '@kerfjs/ui/css-values';
import { describe, expect, it, vi } from 'vitest';

import type { PermissionHistoryItem, PermissionItem } from '../permission-notifications';
import { PermissionRequestCard } from './permission-request-card';
import { ProjectTab } from './project-tab';
import { type TicketLinkChoice, TicketLinkChoiceDialog } from './ticket-link-choice-dialog';
import { TicketRowContextMenu } from './ticket-row-context-menu';

/** The inline color LucideIcon renders on the named icon (Kerf 5.0.0-beta.75 `color` prop). */
function iconColor(markup: string, name: string): string | undefined {
  const svg = new RegExp(`<svg[^>]*data-lucide="${name}"[^>]*>`).exec(markup)?.[0];
  if (!svg) throw new Error(`no ${name} icon`);
  return /style="[^"]*color:([^;"]+)/.exec(svg)?.[1];
}
const token = (name: UiColorName) => String(uiColor(name));

const pending: PermissionItem = {
  id: 7,
  connection: 'claude-main',
  tool: 'Bash',
  action: 'npm test',
  always_allow_supported: true,
  key: 'project:7',
  projectId: 'project',
  projectName: 'Hot Sheet 2',
  agent: 'Claude',
  role: 'main worker',
  receivedAt: 10,
  ignored: false,
};

describe('LucideIcon colors come from the color prop (HS2-GQ57YW)', () => {
  it('tints the permission card summary icon for every request and history state', () => {
    vi.spyOn(Date, 'now').mockReturnValue(10);
    const card = (props: Parameters<typeof PermissionRequestCard>[0]) => String(PermissionRequestCard(props));
    // The agent identity icon keeps its success fill tone in every state.
    expect(iconColor(card({ item: pending }), 'bot')).toBe('var(--wa-color-success-fill-loud)');
    // Pending and resolving requests take the card's accent; failures turn danger.
    expect(iconColor(card({ item: pending }), 'shield-check')).toBe('var(--permission-accent)');
    expect(iconColor(card({ item: pending, state: 'resolving' }), 'clock-3')).toBe('var(--permission-accent)');
    expect(iconColor(card({ item: pending, state: 'failed' }), 'circle-alert')).toBe(token('danger-on-quiet'));
    expect(iconColor(card({ item: pending, state: 'disconnected' }), 'circle-alert')).toBe(token('danger-on-quiet'));
    // History: allowed keeps the accent, denied is danger, externally resolved is neutral.
    const resolved = (decision: PermissionHistoryItem['decision']) =>
      card({ item: { ...pending, decision, resolvedAt: 20 } });
    expect(iconColor(resolved('allow'), 'check')).toBe('var(--permission-accent)');
    expect(iconColor(resolved('deny'), 'x')).toBe(token('danger-on-quiet'));
    expect(iconColor(resolved('external'), 'external-link')).toBe(token('neutral-on-quiet'));
  });

  it('tints context-menu icons by tone and by each metadata choice', () => {
    const markup = String(TicketRowContextMenu({ x: 0, y: 0, category: 'bug', priority: 'high', status: 'started' }));
    // Ordinary actions are quiet, the destructive action is danger.
    expect(iconColor(markup, 'trash-2')).toBe(token('danger-on-quiet'));
    expect(iconColor(markup, 'copy')).toBe(token('neutral-on-quiet'));
    // Category choices carry their own customization colors.
    expect(iconColor(markup, 'bug')).toBe('#ef4444');
    expect(iconColor(markup, 'sparkles')).toBe('#8b5cf6');
  });

  it('tints link-choice status icons by ticket status', () => {
    const match = {
      projectId: 'alpha',
      projectName: 'Alpha',
      ticketId: 'one',
      qualifiedId: 'git:one',
      connectionId: 'git',
      slug: 'HS2-ONE',
      title: 'One',
    };
    const colorFor = (status: string) => {
      const choice: TicketLinkChoice = {
        kind: 'choose',
        reference: { raw: 'HS2-ONE', slug: 'HS2-ONE' },
        matches: [{ ...match, status }],
      };
      return iconColor(String(TicketLinkChoiceDialog({ choice })), 'circle-dot');
    };
    expect(colorFor('started')).toBe(token('brand-on-quiet'));
    expect(colorFor('completed')).toBe(token('success-on-quiet'));
    expect(colorFor('verified')).toBe(token('success-on-quiet'));
    expect(colorFor('not_started')).toBe(token('neutral-on-quiet'));
  });

  it('tints project-tab state icons, and attention wins its own tone', () => {
    const tab = (props: Partial<Parameters<typeof ProjectTab>[0]>) =>
      String(ProjectTab({ id: 'one', name: 'One', location: 'local', ...props }));
    expect(iconColor(tab({ disconnected: true }), 'wifi-off')).toBe(token('neutral-on-quiet'));
    expect(iconColor(tab({ attention: true }), 'circle-alert')).toBe(token('danger-on-quiet'));
  });
});
