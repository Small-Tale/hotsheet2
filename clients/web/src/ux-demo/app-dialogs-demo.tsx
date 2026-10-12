/** Application dialogs rendered from deterministic production state (HS2-HMVBZW). */
import { signal } from 'kerfjs';

import { AiFeedbackDialog } from '../components/ai-feedback-dialog';
import { ConversationExportDialog } from '../components/conversation-export-dialog';
import { ManualModelDialog } from '../components/manual-model-dialog';
import { ProjectCloseDialog } from '../components/project-close-dialog';
import { SavedViewDialog } from '../components/saved-view-dialog';
import { TicketCloseDialog } from '../components/ticket-close-dialog';
import { TicketLinkChoiceDialog } from '../components/ticket-link-choice-dialog';
import type { DemoModule } from './demo-module';
import { savedViewDemoSearchModel } from './ticket-search-field-demo';

// The close dialog's selected running item, so the catalog shows both the chat and the live terminal
// preview (HS2-148B5C).
export const projectCloseDemoSelection = signal('ai-chat:codex-main');

export function TicketCloseDialogDemo() {
  return (
    <TicketCloseDialog
      state={{
        source: {
          id: 'source',
          slug: 'HS2-29MDFH',
          title: 'Repeated UI stability diagnostics',
          projectId: 'kerf',
          projectName: 'Kerf',
          connectionId: 'kerf-git',
          nativeId: 'source',
          qualifiedId: 'kerf-git:source',
        },
        reason: 'duplicate',
        query: 'UI stability',
        candidates: [
          {
            id: 'target',
            slug: 'HS2-8WG3W9',
            title: 'UI stability diagnostics detected render thrashing',
            projectId: 'hotsheet2',
            projectName: 'Hot Sheet 2',
            connectionId: 'hs2-git',
            nativeId: 'target',
            qualifiedId: 'hs2-git:target',
          },
        ],
      }}
    />
  );
}

export function ProjectCloseDialogDemo() {
  return (
    <ProjectCloseDialog
      state={{
        projectId: 'kerf',
        projectName: 'Kerf',
        resources: [
          {
            kind: 'ai-chat',
            id: 'codex-main',
            name: 'Codex',
            tool: 'codex',
            model: 'gpt-5.6-sol-preview',
            effort: 'high',
            progress: 'Reviewing the resize arbiter…',
            messages: [
              {
                id: 'm1',
                role: 'user',
                content: 'Refactor the terminal resize arbiter to remove the hysteresis race.',
                sequence: 0,
              },
              {
                id: 'm2',
                role: 'assistant',
                content:
                  'I split the focus-follow decision from the size claim and added a bounded settle window, so a late viewport claim can no longer shrink an already-committed grid.',
                status: 'completed',
                sequence: 1,
                usage: { tokensIn: 1840, tokensOut: 512, costUsd: 0.021, model: 'gpt-5.6-sol-preview' },
              },
            ],
            activity: [
              {
                id: 'a1',
                tool: 'codex',
                kind: 'read',
                summary: 'Read terminal-sizing.ts',
                importance: 'normal',
                sequence: 2,
              },
            ],
            totalUsage: { tokensIn: 1840, tokensOut: 512, costUsd: 0.021, model: 'gpt-5.6-sol-preview' },
          },
          { kind: 'terminal', id: 'tests', name: 'Tests', cwd: '~/code/kerf', progress: 0.6 },
        ],
        selectedKey: projectCloseDemoSelection.value,
      }}
    />
  );
}

export function ConversationExportDialogDemo() {
  return (
    <ConversationExportDialog
      state={{
        source: {
          conversationId: 'conv-1',
          tool: 'codex',
          sessionId: 'sess-42',
          projectId: 'kerf',
          model: 'gpt-5.6-sol-preview',
          effort: 'high',
          resumable: true,
        },
        messages: [
          {
            id: 'm1',
            role: 'user',
            content: 'Refactor the terminal resize arbiter to remove the hysteresis race.',
            sequence: 0,
          },
          {
            id: 'm2',
            role: 'assistant',
            content: 'I split the focus-follow decision from the size claim and added a bounded settle window.',
            status: 'completed',
            sequence: 1,
            usage: { tokensIn: 1840, tokensOut: 512, costUsd: 0.021, model: 'gpt-5.6-sol-preview' },
          },
          { id: 'm3', role: 'user', content: 'Add a regression test for the late-claim case.', sequence: 2 },
        ],
        draft: {
          scope: { kind: 'all' },
          writeMode: 'create',
          bundle: { includeAttachments: true, includeMedia: true, includeSummary: true },
          destination: {
            selectionToken: 'sel-token',
            displayPath: '~/exports/kerf-resize-arbiter',
            kind: 'directory',
          },
        },
        step: 2,
        summaryAvailable: true,
      }}
    />
  );
}

export function SavedViewDialogDemo() {
  return <SavedViewDialog open mode="create" name="Blocked bugs" searchModel={savedViewDemoSearchModel} />;
}

export function TicketLinkChoiceDialogDemo() {
  return (
    <TicketLinkChoiceDialog
      choice={{
        kind: 'choose',
        reference: { raw: 'HS2-DEMO01', slug: 'HS2-DEMO01' },
        matches: [
          {
            projectId: 'hotsheet2',
            projectName: 'Hot Sheet 2',
            ticketId: '01',
            qualifiedId: 'hs2-git:01',
            connectionId: 'hs2-git',
            slug: 'HS2-DEMO01',
            title: 'Use real project tickets',
            status: 'started',
          },
          {
            projectId: 'kerf',
            projectName: 'Kerf',
            ticketId: 'k1',
            qualifiedId: 'kerf-git:k1',
            connectionId: 'kerf-git',
            slug: 'HS2-DEMO01',
            title: 'Mirror the ticket reference in Kerf',
            status: 'not_started',
          },
        ],
      }}
    />
  );
}

export function AiFeedbackDialogDemo() {
  return (
    <AiFeedbackDialog
      state={{
        ticketId: 'HS2-DEMO01',
        target: 'note:example',
        rating: 'not_helpful',
        rater: 'demo',
        explanation: 'Please explain the failed step and suggest a smaller fix.',
        revising: false,
      }}
    />
  );
}

export function ManualModelDialogDemo() {
  return <ManualModelDialog state={{ target: 'settings', providerName: 'Codex', value: 'gpt-5.6-sol-preview' }} />;
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'ticket-close-dialog': { Demo: TicketCloseDialogDemo },
    'project-close-dialog': { Demo: ProjectCloseDialogDemo },
    'conversation-export-dialog': { Demo: ConversationExportDialogDemo },
    'saved-view-dialog': { Demo: SavedViewDialogDemo },
    'ticket-link-choice-dialog': { Demo: TicketLinkChoiceDialogDemo },
    'ai-feedback-dialog': { Demo: AiFeedbackDialogDemo },
    'manual-model-dialog': { Demo: ManualModelDialogDemo },
  },
};
