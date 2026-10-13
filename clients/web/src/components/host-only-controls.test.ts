import { describe, expect, it } from 'vitest';

import type { CodeReview, CorruptTicket } from '../api';
import { hostOnlyActionMessage, hostOnlyDisabledReason } from '../host-only-actions';
import { AIConversation } from './ai-conversation';
import { AttachmentContextMenu } from './attachment-context-menu';
import { corruptTicketInspectorPanel } from './corrupt-ticket-row';
import { Hs1MigrationDialog } from './hs1-migration';
import { ProjectDialog } from './project-dialog';
import { ChangeEvidenceDialog, RepositoryStatusPopover } from './repository-status-popover';
import { TerminalDrawer } from './terminal-drawer';
import { TicketCodeReview } from './ticket-code-review';
import { TicketSourceSetupDialog } from './ticket-source-setup-dialog';

/** HS2-TYS5SS: host-only controls render disabled, with the reason, before a remote user clicks. */
const escape = (text: string) => text.replaceAll("'", '&#39;');
const reason = (action: Parameters<typeof hostOnlyActionMessage>[0]) => escape(hostOnlyActionMessage(action));
const review: CodeReview = {
  difftool: 'Meld',
  truncated: false,
  ranges: [{ from: 'aaa1111', to: 'bbb2222', count: 2 }],
  commits: [
    { sha: 'bbb2222', short_sha: 'bbb2222', subject: 'Second', committed_at: '2026-09-02T10:00:00Z' },
    { sha: 'aaa1111', short_sha: 'aaa1111', subject: 'First', committed_at: '2026-09-02T09:00:00Z' },
  ],
};
const menuItem = (markup: string, id: string) =>
  markup.match(new RegExp(`<wa-dropdown-item[^>]*data-(?:item-id|repository-file-action|action)="${id}"[^>]*>`))?.[0] ??
  '';

describe('host-only controls on remote clients', () => {
  it('reports a reason only when the action is unavailable', () => {
    expect(hostOnlyDisabledReason('diff-tool', true)).toBeUndefined();
    expect(hostOnlyDisabledReason('diff-tool', false)).toBe(hostOnlyActionMessage('diff-tool'));
  });

  it('disables every Code Review diff-tool action with the explanation', () => {
    const remote = String(
      TicketCodeReview({
        review,
        comparison: { active: true, side: 'b', a: 'aaa1111', b: 'bbb2222' },
        hostActionsAvailable: false,
      }),
    );
    const buttons = remote.match(/<button[^>]*data-review-mode="[^"]+"[^>]*>/g) ?? [];
    expect(buttons).toHaveLength(4);
    for (const button of buttons) {
      expect(button).toContain('disabled');
      expect(button).toContain(`title="${reason('diff-tool')}"`);
    }
    // A configured diff tool is not reported missing just because this device cannot launch it.
    expect(remote).not.toContain('No Git diff tool is configured');
    const local = String(TicketCodeReview({ review, hostActionsAvailable: true }));
    for (const button of local.match(/<button[^>]*data-review-mode="[^"]+"[^>]*>/g) ?? [])
      expect(button).not.toContain('disabled');
  });

  it('passes the remote state through the repository popover and change evidence menus', () => {
    const fileMenu = { path: 'src/a.ts', x: 1, y: 1, diff: 'unstaged' as const };
    const repository = String(
      RepositoryStatusPopover({
        status: null,
        initialized: true,
        view: 'commits',
        fileMenu: { ...fileMenu, source: 'repository' },
        detailCommits: review.commits,
        hostActionsAvailable: false,
      }),
    );
    for (const action of ['open', 'reveal']) expect(menuItem(repository, action)).toContain('disabled');
    expect(menuItem(repository, 'open')).toContain(reason('open-file'));
    expect(menuItem(repository, 'reveal')).toContain(reason('reveal-file'));
    expect(menuItem(repository, 'show-diff')).not.toContain('disabled');
    const evidence = String(
      ChangeEvidenceDialog({ review, fileMenu: { ...fileMenu, source: 'ticket' }, hostActionsAvailable: false }),
    );
    expect(menuItem(evidence, 'reveal')).toContain(reason('reveal-file'));
  });

  it('disables attachment Open and reveal but keeps download and copy actions', () => {
    const markup = String(AttachmentContextMenu({ x: 1, y: 1, hostActionsAvailable: false }));
    expect(menuItem(markup, 'open')).toContain(reason('open-file'));
    expect(menuItem(markup, 'reveal')).toContain(reason('reveal-file'));
    expect(menuItem(markup, 'download')).not.toContain('disabled');
    expect(String(AttachmentContextMenu({ x: 1, y: 1, hostActionsAvailable: true }))).not.toContain(
      reason('open-file'),
    );
  });

  it('disables corrupt-ticket reveal and the folder choosers', () => {
    const ticket: CorruptTicket = {
      store: 'git',
      store_path: '/store',
      path: '/store/tickets/x.md',
      error: 'bad',
      error_code: 'invalid_ticket',
    };
    const recovery = String(corruptTicketInspectorPanel({ ticket, hostActionsAvailable: false }).content);
    expect(recovery).toMatch(
      new RegExp(`data-action="reveal-corrupt-ticket"[^>]*disabled[^>]*title="${reason('reveal-file')}"`),
    );
    const project = String(ProjectDialog({ open: true, root: '/work', error: '', hostActionsAvailable: false }));
    expect(project.match(/data-action="browse-project-path"[^>]*>/g)?.join('')).toContain(reason('choose-folder'));
    expect((project.match(/data-action="browse-project-path"[^>]*disabled/g) ?? []).length).toBe(2);
    const hs1 = String(
      Hs1MigrationDialog({
        projectName: 'Demo',
        projectRoot: '/work/demo',
        sourcePath: '/work/demo/.hotsheet',
        databasePath: '/work/demo/.hotsheet/db',
        defaultStore: '/work/demo.hs2',
        open: true,
        busy: false,
        hostActionsAvailable: false,
      }),
    );
    expect(hs1).toMatch(
      new RegExp(`data-action="browse-hs1-ticket-store"[^>]*disabled[^>]*title="${reason('choose-folder')}"`),
    );
    const setup = String(
      TicketSourceSetupDialog({
        project: { root: '/work/demo', name: 'Demo', stores: [], needsTicketSetup: true },
        providerConnections: [],
        navigation: 'none',
        hostActionsAvailable: false,
      }),
    );
    expect(setup).toContain(reason('choose-folder'));
  });

  it('disables conversation save and the saved-conversation opener', () => {
    const conversation = String(
      AIConversation({
        open: true,
        tool: 'Codex',
        draft: '',
        busy: false,
        interruptible: false,
        messages: [{ id: 'answer', role: 'assistant', content: 'Done.', status: 'completed' }],
        hostActionsAvailable: false,
      }),
    );
    expect(conversation).toMatch(/aria-label="Save conversation"/);
    expect(conversation).toContain(`title="${reason('conversation-export')}"`);
    const drawer = String(
      TerminalDrawer({
        projectId: 'project',
        projectName: 'Project',
        sessions: [],
        width: 900,
        height: 320,
        fitAcross: 1,
        fitHigh: 1,
        selectedId: 'grid',
        hostActionsAvailable: false,
      }),
    );
    expect(menuItem(drawer, 'open-saved-conversation')).toContain(reason('conversation-open'));
  });
});
