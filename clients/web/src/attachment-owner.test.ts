import { describe, expect, it } from 'vitest';

import type { FullTicket } from './api';
import { attachmentOwnerTarget } from './attachment-owner';
import type { TicketReaderFrame } from './ticket-reader-stack';

const ticket = (qualified_id: string) => ({ qualified_id, slug: qualified_id }) as unknown as FullTicket;
const frame = (id: string, capabilities: { attachments: boolean; attachment_edit?: boolean }) =>
  ({
    id,
    projectId: `${id}-project`,
    apiPath: `/__hotsheet/project-api/${id}-project`,
    ticket: ticket(`git-${id}:${id}`),
    capabilities,
  }) as unknown as TicketReaderFrame;

describe('attachment action ticket selection (HS2-KKB9VF)', () => {
  const workspace = { projectId: 'source-project', ticket: ticket('git-source:source') };
  const linked = frame('linked', { attachments: true, attachment_edit: true }),
    appendOnly = frame('append-only', { attachments: true, attachment_edit: false }),
    legacy = frame('legacy', { attachments: true }),
    readOnly = frame('read-only', { attachments: false });
  const frames = [linked, appendOnly, legacy, readOnly];

  it('routes a linked frame action to that frame, never the workspace selection', () => {
    expect(attachmentOwnerTarget('linked', frames, workspace)).toEqual({
      kind: 'frame',
      frame: linked,
      canUse: true,
      canEdit: true,
    });
    // The frame's own ticket and project, even though a workspace ticket is selected.
    const target = attachmentOwnerTarget('append-only', frames, workspace);
    expect(target?.kind === 'frame' && [target.frame.projectId, target.frame.ticket.qualified_id]).toEqual([
      'append-only-project',
      'git-append-only:append-only',
    ]);
  });

  it('takes each frame provider capability: append-only, legacy (no attachment_edit), and read-only', () => {
    expect(attachmentOwnerTarget('append-only', frames, workspace)).toMatchObject({ canUse: true, canEdit: false });
    expect(attachmentOwnerTarget('legacy', frames, workspace)).toMatchObject({ canUse: true, canEdit: true });
    expect(attachmentOwnerTarget('read-only', frames, workspace)).toMatchObject({ canUse: false, canEdit: false });
  });

  it('routes an action outside any linked frame to the workspace selection', () => {
    expect(attachmentOwnerTarget(undefined, frames, workspace)).toEqual({
      kind: 'workspace',
      projectId: 'source-project',
      ticket: workspace.ticket,
    });
    expect(attachmentOwnerTarget('', frames, workspace)).toMatchObject({ kind: 'workspace' });
    // No project or no selected ticket: nothing to act on.
    expect(attachmentOwnerTarget(undefined, frames, { ticket: workspace.ticket })).toBeUndefined();
    expect(attachmentOwnerTarget(undefined, frames, { projectId: 'source-project', ticket: null })).toBeUndefined();
  });

  it('resolves a closed frame to nothing instead of falling through to the workspace ticket', () => {
    // Open, then close the frame: a late gallery or menu action for it must not hit the workspace.
    expect(attachmentOwnerTarget('linked', frames, workspace)).toMatchObject({ kind: 'frame' });
    expect(attachmentOwnerTarget('linked', [appendOnly], workspace)).toBeUndefined();
    expect(attachmentOwnerTarget('linked', [], workspace)).toBeUndefined();
    // Reopened (a new frame with the same id) routes to it again.
    expect(attachmentOwnerTarget('linked', [linked], workspace)).toMatchObject({ kind: 'frame', frame: linked });
  });
});
