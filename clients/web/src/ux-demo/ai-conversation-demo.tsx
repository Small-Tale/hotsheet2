import { CatalogExample } from '@kerfjs/ui/catalog';
import { List } from '@kerfjs/ui/list';
import { Select } from '@kerfjs/ui/select';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import {
  type ConversationActivity,
  type ConversationMessage,
  type ConversationTurnProgress,
  conversationTurnProgress,
  conversationUsage,
} from '../ai-conversation';
import { AIConversation, type AIConversationProps, AIConversationToolbar } from '../components/ai-conversation';
import { wireAiConversation } from '../components/ai-conversation-interactions';
import {
  type ConversationHistoryCursor,
  conversationSentHistory,
  stepConversationHistory,
} from '../conversation-history';
import {
  type ConversationMessageSelection,
  conversationSelectionAfterPick,
  EMPTY_CONVERSATION_SELECTION,
  selectedConversationMessagesFor,
} from '../conversation-selection';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import type { PermissionItem } from '../permission-notifications';
import { DemoEvent } from './components/demo-event';
import { type DemoDevice, DeviceSetting } from './components/device-setting';
import { DEMO_ACTIONS } from './demo-actions';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { sidebarEvent } from './project-sidebar-demo';
import { syncSettingsControls } from './settings-controls';

export type AIConversationScenario =
  | 'empty'
  | 'streaming'
  | 'running-tool'
  | 'stuck'
  | 'permission'
  | 'completed'
  | 'usage-unpriced'
  | 'failed'
  | 'interrupted'
  | 'saved';
export const aiConversationScenario = signal<AIConversationScenario>('streaming');
/** Remote clients disable the host-only export and saved-conversation actions (HS2-NWWF1Z). */
export const aiConversationDevice = signal<DemoDevice>('host');
export const aiConversationDemoOpen = signal(true);
export const aiConversationPresentation = signal<'dialog' | 'embedded'>('dialog');
export const aiConversationSaveCount = signal(0);
/** Transcript selection, driven by the production plain / Cmd-Ctrl / Shift pick rules (HS2-WV88W3). */
export const aiConversationSelection = signal<ConversationMessageSelection>(EMPTY_CONVERSATION_SELECTION);
function demoSelectedMessageIds(messages: readonly ConversationMessage[]): string[] {
  return selectedConversationMessagesFor(messages, aiConversationSelection.value).map((message) => message.id);
}
export const aiConversationDraft = signal('Can you also explain the compatibility boundary?');
export const aiConversationProvider = signal('codex');
export const AI_CONVERSATION_PROVIDERS = [
  { id: 'codex', label: 'Codex' },
  { id: 'claude', label: 'Claude' },
] as const;
export function aiConversationProviderLabel(id = aiConversationProvider.value) {
  return AI_CONVERSATION_PROVIDERS.find((item) => item.id === id)?.label ?? id;
}

const completed: ConversationMessage[] = [
  {
    id: 'question-1',
    role: 'user',
    sequence: 0,
    content: 'Review the client connection flow in `api.ts`.',
    // Referenced files render as Kerf link Chips on both bubble tones (HS2-94T958).
    files: [
      {
        id: 'api-ts',
        filename: 'api.ts',
        mime_type: 'text/typescript',
        kind: 'attachment',
        url: '/ux-demo/fixtures/api.ts',
      },
    ],
  },
  {
    id: 'answer-1',
    role: 'assistant',
    // The turn's tool activity lands between the question and its answer.
    sequence: 3,
    status: 'completed',
    content:
      'The connection uses the project event stream, so the transcript updates without simple polling.\n\n- Turns stay in one session.\n- Permissions appear inline.',
    usage: { tokensIn: 18_234, tokensOut: 2_101, costUsd: 0.0423, model: 'codex-5.6' },
    files: [
      {
        id: 'connection-diagram',
        filename: 'connection-flow-diagram-with-a-long-descriptive-name.png',
        mime_type: 'image/png',
        kind: 'media',
        url: '/ux-demo/fixtures/connection-flow.png',
      },
    ],
  },
];
const activity: ConversationActivity[] = [
  {
    id: 'activity-1',
    tool: 'Codex',
    sequence: 1,
    kind: 'edit',
    summary: 'Edited the conversation state boundary',
    importance: 'normal',
  },
  {
    id: 'activity-2',
    tool: 'Codex',
    sequence: 2,
    kind: 'command',
    summary:
      "Codex ran `zsh -ic 'npm run test -- --project chromium --grep ai-conversation-activity-presentation-with-a-deliberately-long-filter'`",
    importance: 'normal',
  },
];
const permission: PermissionItem = {
  id: 42,
  connection: 'hotsheet-sidebar',
  tool: 'Bash',
  action: 'npm run test',
  always_allow_supported: true,
  key: 'demo:42',
  projectId: 'demo',
  projectName: 'Hot Sheet 2',
  agent: 'Codex',
  role: 'main worker',
  receivedAt: Date.now(),
  ignored: false,
};

function scenarioState() {
  const scenario = aiConversationScenario.value;
  if (scenario === 'empty') return { messages: [] as ConversationMessage[], busy: false, interruptible: false };
  if (scenario === 'saved') return { messages: completed, busy: false, interruptible: false };
  if (scenario === 'completed') return { messages: completed, busy: false, interruptible: false, activity };
  if (scenario === 'usage-unpriced')
    return {
      messages: completed.map((message) =>
        message.role === 'assistant'
          ? { ...message, usage: { tokensIn: 8_420, tokensOut: 943, model: 'unpriced-model' } }
          : message,
      ),
      busy: false,
      interruptible: false,
      activity,
    };
  if (scenario === 'interrupted')
    return {
      messages: [
        ...completed,
        { id: 'question-2', role: 'user' as const, content: 'Run the full suite.' },
        {
          id: 'answer-2',
          role: 'assistant' as const,
          status: 'interrupted' as const,
          content: 'Stopped before the suite completed.',
        },
      ],
      busy: false,
      interruptible: false,
    };
  if (scenario === 'failed')
    return {
      messages: [
        ...completed,
        { id: 'question-2', role: 'user' as const, content: 'Inspect the server.' },
        {
          id: 'answer-2',
          role: 'assistant' as const,
          status: 'failed' as const,
          content: 'The turn ended without output.',
        },
      ],
      busy: false,
      interruptible: false,
      error: 'The tool turn failed.',
    };
  const messages = [
    ...completed,
    { id: 'question-2', role: 'user' as const, content: 'Run the focused browser test.' },
    {
      id: 'answer-2',
      role: 'assistant' as const,
      status: 'streaming' as const,
      content: scenario === 'streaming' ? 'I found the production route and I’m checking its' : '',
    },
  ];
  return {
    messages,
    busy: true,
    interruptible: true,
    progress: scenario === 'permission' ? 'Waiting for permission…' : 'Running the focused browser test…',
    permissions: scenario === 'permission' ? [permission] : undefined,
    turnProgress: demoTurnProgress(scenario),
  };
}

/**
 * Deterministic running-turn progress (HS2-XV1VCR) through the production derivation: a fixed
 * clock stands in for the app's local render clock, and a fixed connection snapshot for the server.
 */
const DEMO_TURN_START = Date.UTC(2026, 9, 16, 9, 0, 0);
function demoTurnProgress(scenario: AIConversationScenario): ConversationTurnProgress | undefined {
  const tool = scenario === 'running-tool' || scenario === 'stuck',
    // Streaming: 3m 12s in, active now. Tool: Bash open for 2m 5s, last event 8s ago. Stuck: 6m silent.
    elapsed = scenario === 'stuck' ? 9 * 60_000 : 192_000,
    lastEventAt =
      DEMO_TURN_START + (scenario === 'stuck' ? 3 * 60_000 : scenario === 'running-tool' ? 184_000 : elapsed),
    connection = {
      busy: true,
      turn_id: 'demo-turn',
      turn_started_at_ms: DEMO_TURN_START,
      last_event_at_ms: lastEventAt,
      in_flight_tool: tool ? { name: 'Bash', started_at_ms: DEMO_TURN_START + 67_000, count: 1 } : undefined,
    };
  return conversationTurnProgress({ messages: [] }, connection, DEMO_TURN_START + elapsed, {
    waitingForPermission: scenario === 'permission',
  });
}

export function AIConversationDemo() {
  const state = scenarioState();
  const embedded = aiConversationPresentation.value === 'embedded';
  const conversation: AIConversationProps = {
    presentation: aiConversationPresentation.value,
    open: aiConversationDemoOpen.value,
    tool: aiConversationProviderLabel(),
    sessionId: '019-demo-session',
    selectionId: 'demo-conversation',
    selectedMessageIds: demoSelectedMessageIds(state.messages),
    messages: state.messages,
    // The saved transcript reopens a non-contiguous selection that left three messages out (HS2-BZB93V).
    skippedBefore: aiConversationScenario.value === 'saved' ? { 'answer-1': 3 } : undefined,
    draft: aiConversationDraft.value,
    busy: state.busy,
    progress: state.progress,
    turnProgress: 'turnProgress' in state ? state.turnProgress : undefined,
    interruptible: state.interruptible,
    permissions: state.permissions,
    activity: state.activity,
    totalUsage: conversationUsage({ messages: state.messages }),
    error: state.error,
    feedbackAvailable: true,
    providerId: aiConversationProvider.value,
    providers: AI_CONVERSATION_PROVIDERS,
    canChangeProvider: true,
    model: 'gpt-5.6',
    effort: 'high',
    models: [
      { id: 'gpt-5.6', label: 'GPT-5.6' },
      { id: 'gpt-5.6-codex', label: 'GPT-5.6 Codex' },
    ],
    efforts: ['medium', 'high', 'xhigh'],
    canChangeModel: true,
    canChangeEffort: true,
    readOnly: aiConversationScenario.value === 'saved',
    savedSource: aiConversationScenario.value === 'saved' ? 'client-review.hotsheet-chat' : undefined,
    hostActionsAvailable: aiConversationDevice.value === 'host',
  };
  // Embedded, the Kerf app-height viewport is the definite height the conversation fills; the List
  // stack owns the status row and the flexing conversation (HS2-H1DRJB), so no demo stage CSS remains.
  return (
    <CatalogExample label="Interactive" viewport={embedded ? { width: 'full', height: 'app' } : { width: 'full' }}>
      <List
        fill={embedded}
        gap="xs"
        className="ai-conversation-demo"
        rootAttributes={{
          role: 'region',
          'aria-label': 'AIConversation demo',
          'data-presentation': aiConversationPresentation.value,
        }}
      >
        {aiConversationPresentation.value === 'dialog' && (
          <wa-button appearance="accent" data-action="open-ai-conversation-demo">
            Open Conversation
          </wa-button>
        )}
        <DemoEvent>
          {aiConversationSaveCount.value
            ? `Prepared ${aiConversationSaveCount.value} conversation export${aiConversationSaveCount.value === 1 ? '' : 's'}.`
            : ''}
        </DemoEvent>
        <List flex={embedded}>
          {/* The drawer renders these controls in its tab toolbar; this compact Kerf Toolbar stands in
              for that row so the embedded demo exposes every control (HS2-59F7WK). */}
          {embedded && (
            <Toolbar
              density="compact"
              dividerSides="b"
              leading={<ToolbarText text="Drawer tab toolbar" size="small" />}
              trailing={<AIConversationToolbar {...conversation} />}
            />
          )}
          <AIConversation {...conversation} />
        </List>
      </List>
    </CatalogExample>
  );
}

/** Restore the canonical conversation settings and sync the live controls (HS2-X1SM48). */
export function resetAIConversationDemo(root?: ParentNode): void {
  aiConversationPresentation.value = 'dialog';
  aiConversationScenario.value = 'streaming';
  aiConversationSelection.value = EMPTY_CONVERSATION_SELECTION;
  aiConversationDevice.value = 'host';
  aiConversationDemoOpen.value = true;
  if (root)
    syncSettingsControls(root, 'ai-conversation', {
      values: {
        presentation: aiConversationPresentation.value,
        scenario: aiConversationScenario.value,
        device: aiConversationDevice.value,
      },
    });
}

export function AIConversationSettings() {
  return (
    <DemoSettingsForm settings="ai-conversation">
      <Select
        name="presentation"
        label="Presentation"
        value={aiConversationPresentation.value}
        choices={[
          { value: 'dialog', label: 'Dialog' },
          { value: 'embedded', label: 'Embedded' },
        ]}
      />
      <Select
        name="scenario"
        label="Public state"
        value={aiConversationScenario.value}
        choices={[
          { value: 'empty', label: 'Empty' },
          { value: 'streaming', label: 'Streaming' },
          { value: 'running-tool', label: 'Running a Tool' },
          { value: 'stuck', label: 'May Be Stuck' },
          { value: 'permission', label: 'Permission Requested' },
          { value: 'completed', label: 'Completed with Priced Usage' },
          { value: 'usage-unpriced', label: 'Completed with Unpriced Usage' },
          { value: 'failed', label: 'Failed' },
          { value: 'interrupted', label: 'Interrupted' },
          { value: 'saved', label: 'Saved Read-Only Transcript' },
        ]}
      />
      <DeviceSetting value={aiConversationDevice.value} />
      {aiConversationPresentation.value === 'dialog' && (
        <wa-button type="button" data-action="open-ai-conversation-demo">
          Open Conversation
        </wa-button>
      )}
      <wa-button type="button" data-action="reset-settings">
        Reset
      </wa-button>
    </DemoSettingsForm>
  );
}

/** Open the conversation surface, waiting a microtask for the controlled render to mount it. */
export function openAIConversationDemo(root: HTMLElement): void {
  aiConversationDemoOpen.value = true;
  queueMicrotask(() => {
    root.querySelector<HTMLElement & { show?(): void }>('[data-component="ai-conversation"]')?.show?.();
  });
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireAiConversationDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  let demoHistoryCursor: ConversationHistoryCursor | undefined;
  demoListeners.add(
    delegate(root, 'change', '[data-settings="ai-conversation"] [name="presentation"]', (_event, target) => {
      aiConversationPresentation.value = (target as FormControl).value as typeof aiConversationPresentation.value;
      openAIConversationDemo(root);
    }),
  );
  demoListeners.add(
    delegate(root, 'change', '[data-settings="ai-conversation"] [name="scenario"]', (_event, target) => {
      aiConversationScenario.value = (target as FormControl).value as typeof aiConversationScenario.value;
      aiConversationSelection.value = EMPTY_CONVERSATION_SELECTION;
      openAIConversationDemo(root);
    }),
  );
  demoListeners.add(
    delegate(root, 'change', '[data-settings="ai-conversation"] [name="device"]', (_event, target) => {
      aiConversationDevice.value = (target as FormControl).value as DemoDevice;
      openAIConversationDemo(root);
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.openAiConversationDemo.selector, () => {
      openAIConversationDemo(root);
    }),
  );
  // The selection actions use the production action attributes; the demo keeps its own range.
  demoListeners.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.clearConversationSelection.selector, () => {
      aiConversationSelection.value = EMPTY_CONVERSATION_SELECTION;
    }),
  );
  demoListeners.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.copyConversationSelection.selector, () => {
      sidebarEvent.value = `Copied ${demoSelectedMessageIds(scenarioState().messages).length} selected messages.`;
    }),
  );
  // The production AIConversation handlers, driving the fixture scenario (HS2-RWY78R).
  demoListeners.add(
    wireAiConversation(root, {
      onHidden: () => {
        aiConversationDemoOpen.value = false;
      },
      save: () => {
        aiConversationSaveCount.value++;
      },
      pickMessage: (_connectionId, messageId, modifiers) => {
        aiConversationSelection.value = conversationSelectionAfterPick(
          scenarioState().messages,
          aiConversationSelection.value,
          messageId,
          modifiers,
        );
        sidebarEvent.value = `Picked conversation message ${messageId}.`;
      },
      setDraft: (value) => {
        demoHistoryCursor = undefined;
        aiConversationDraft.value = value;
      },
      recallHistory: (direction, currentValue) => {
        const step = stepConversationHistory(
          conversationSentHistory(scenarioState().messages),
          demoHistoryCursor,
          direction,
          currentValue,
        );
        if (!step) return;
        demoHistoryCursor = step.cursor;
        aiConversationDraft.value = step.value;
        return step.value;
      },
      send: () => {
        aiConversationScenario.value = 'streaming';
      },
      stop: () => {
        aiConversationScenario.value = 'interrupted';
      },
      selectProvider: (id) => {
        if (!id) return;
        aiConversationProvider.value = id;
        sidebarEvent.value = `Switched conversation provider to ${aiConversationProviderLabel(id)}.`;
      },
      selectModel: (model) => {
        sidebarEvent.value = `Conversation model set to ${model}.`;
      },
      selectEffort: (effort) => {
        sidebarEvent.value = `Conversation effort set to ${effort}.`;
      },
      openManualModel: () => {
        sidebarEvent.value = 'Manual conversation model entry requested.';
      },
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}
/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'ai-conversation': {
      catalogExamples: true,
      Demo: AIConversationDemo,
      Settings: AIConversationSettings,
      reset: (root) => {
        resetAIConversationDemo(root);
        openAIConversationDemo(root);
      },
    },
  },
  wire: wireAiConversationDemo,
};
