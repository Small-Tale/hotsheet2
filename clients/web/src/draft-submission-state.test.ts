import { describe, expect, it } from 'vitest';

import { finishDraftSubmission } from './draft-submission-state';
import {
  loadProjectWorkspaceSession,
  type ProjectWorkspaceSession,
  saveProjectWorkspaceSession,
} from './workspace-session';

function fixture() {
  const entries = new Map<string, string>(),
    storage = {
      getItem: (key: string) => entries.get(key) ?? null,
      setItem: (key: string, value: string) => {
        entries.set(key, value);
      },
    };
  const session: ProjectWorkspaceSession = {
    selectedView: 'all',
    selectedTicketSlugs: [],
    searchOpen: false,
    searchQuery: '',
    inspectorTab: 'info',
    readerTab: 'info',
    composingNote: false,
    newNoteDraft: '',
    feedbackReplies: {},
    feedbackSelections: {},
    feedbackDraft: '',
    composer: {
      open: true,
      title: 'Synthetic draft',
      details: '',
      category: 'task',
      upNext: false,
      attachments: [{ id: 'old', name: 'old.bin' }],
    },
    notWorking: {
      ticketId: 'one',
      slug: 'HS-SYNTHETIC',
      connectionId: 'local',
      note: 'Synthetic report',
      attachments: [{ id: 'old', name: 'old.bin' }],
    },
  };
  return { storage, session };
}

describe('HS2-791W19 submitted origin draft cleanup', () => {
  it('clears the matching origin draft without changing another project', () => {
    const { storage, session } = fixture();
    saveProjectWorkspaceSession(storage, 'a', session);
    saveProjectWorkspaceSession(storage, 'b', session);
    const before = loadProjectWorkspaceSession(storage, 'b');
    finishDraftSubmission(storage, 'a', { kind: 'composer', draft: session.composer });
    expect(loadProjectWorkspaceSession(storage, 'a')?.composer).toMatchObject({
      title: '',
      open: false,
      attachments: [],
    });
    expect(loadProjectWorkspaceSession(storage, 'b')).toEqual(before);
  });
  it('removes submitted ids while preserving a later composer edit and new refs', () => {
    const { storage, session } = fixture();
    saveProjectWorkspaceSession(storage, 'a', {
      ...session,
      composer: {
        ...session.composer,
        title: 'Later draft',
        attachments: [...session.composer.attachments, { id: 'new', name: 'new.bin' }],
      },
    });
    finishDraftSubmission(storage, 'a', { kind: 'composer', draft: session.composer });
    expect(loadProjectWorkspaceSession(storage, 'a')?.composer).toMatchObject({
      title: 'Later draft',
      open: true,
      attachments: [{ id: 'new', name: 'new.bin' }],
    });
  });
  it('keeps later report text/new refs or another target, and clears only an unchanged report', () => {
    const { storage, session } = fixture(),
      submitted = session.notWorking!;
    saveProjectWorkspaceSession(storage, 'a', {
      ...session,
      notWorking: {
        ...submitted,
        note: 'Later report',
        attachments: [...submitted.attachments, { id: 'new', name: 'new.bin' }],
      },
    });
    finishDraftSubmission(storage, 'a', { kind: 'not-working', draft: submitted });
    expect(loadProjectWorkspaceSession(storage, 'a')?.notWorking).toMatchObject({
      note: 'Later report',
      attachments: [{ id: 'new', name: 'new.bin' }],
    });
    saveProjectWorkspaceSession(storage, 'a', { ...session, notWorking: { ...submitted, ticketId: 'two' } });
    finishDraftSubmission(storage, 'a', { kind: 'not-working', draft: submitted });
    expect(loadProjectWorkspaceSession(storage, 'a')?.notWorking?.ticketId).toBe('two');
    saveProjectWorkspaceSession(storage, 'a', { ...session, notWorking: { ...submitted, mode: 'reopen' } });
    finishDraftSubmission(storage, 'a', { kind: 'not-working', draft: submitted });
    expect(loadProjectWorkspaceSession(storage, 'a')?.notWorking?.mode).toBe('reopen');
    saveProjectWorkspaceSession(storage, 'a', session);
    finishDraftSubmission(storage, 'a', { kind: 'not-working', draft: submitted });
    expect(loadProjectWorkspaceSession(storage, 'a')?.notWorking).toBeUndefined();
    finishDraftSubmission(storage, 'missing', { kind: 'not-working', draft: submitted });
  });
});
