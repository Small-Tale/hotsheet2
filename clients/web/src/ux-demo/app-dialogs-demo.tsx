import { CatalogExample } from '@kerfjs/ui/catalog';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { AiFeedbackDialog } from '../components/ai-feedback-dialog';
import { ConversationExportDialog } from '../components/conversation-export-dialog';
import { ManualModelDialog } from '../components/manual-model-dialog';
import { focusProjectCloseDialogDefault, ProjectCloseDialog } from '../components/project-close-dialog';
import { SavedViewDialog } from '../components/saved-view-dialog';
import { TicketCloseDialog } from '../components/ticket-close-dialog';
import { TicketLinkChoiceDialog } from '../components/ticket-link-choice-dialog';
import type { DemoModule } from './demo-module';
import { savedViewDemoSearchModel, ticketSearchDemoHelpIsOpen } from './ticket-search-field-demo';

// The close dialog's selected running item, so the catalog shows both the chat and the live terminal
// preview (HS2-148B5C).
export const projectCloseDemoSelection = signal('ai-chat:codex-main');

export function TicketCloseDialogDemo() {
  return (
    <CatalogExample label="Interactive">
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
    </CatalogExample>
  );
}

export function ProjectCloseDialogDemo() {
  return (
    <CatalogExample label="Interactive">
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
    </CatalogExample>
  );
}

export function ConversationExportDialogDemo() {
  return (
    <CatalogExample label="Interactive">
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
    </CatalogExample>
  );
}

export function SavedViewDialogDemo() {
  return (
    <CatalogExample label="Interactive">
      <SavedViewDialog
        open
        mode="create"
        name="Blocked bugs"
        searchModel={savedViewDemoSearchModel}
        helpOpen={ticketSearchDemoHelpIsOpen('saved-view-query')}
      />
    </CatalogExample>
  );
}

export function TicketLinkChoiceDialogDemo() {
  return (
    <CatalogExample label="Interactive">
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
    </CatalogExample>
  );
}

export function AiFeedbackDialogDemo() {
  return (
    <CatalogExample label="Interactive">
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
    </CatalogExample>
  );
}

export function ManualModelDialogDemo() {
  return (
    <CatalogExample label="Interactive">
      <ManualModelDialog state={{ target: 'settings', providerName: 'Codex', value: 'gpt-5.6-sol-preview' }} />
    </CatalogExample>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireAppDialogsDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  // The production presenter moves the opened dialog's focus to its safe default (HS2-9YGNMX).
  focusProjectCloseDialogDefault(root);
  demoListeners.add(
    delegate(
      root,
      'click',
      '[data-dialog-component="project-close-dialog"] [data-action="select-project-close-resource"]',
      (_event, target) => {
        const key = target.getAttribute('data-item-id');
        if (key) projectCloseDemoSelection.value = key;
      },
    ),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders: application dialogs rendered from deterministic production state (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'ticket-close-dialog': { catalogExamples: true, Demo: TicketCloseDialogDemo },
    'project-close-dialog': { catalogExamples: true, Demo: ProjectCloseDialogDemo },
    'conversation-export-dialog': { catalogExamples: true, Demo: ConversationExportDialogDemo },
    'saved-view-dialog': { catalogExamples: true, Demo: SavedViewDialogDemo },
    'ticket-link-choice-dialog': { catalogExamples: true, Demo: TicketLinkChoiceDialogDemo },
    'ai-feedback-dialog': { catalogExamples: true, Demo: AiFeedbackDialogDemo },
    'manual-model-dialog': { catalogExamples: true, Demo: ManualModelDialogDemo },
  },
  wire: wireAppDialogsDemo,
};
