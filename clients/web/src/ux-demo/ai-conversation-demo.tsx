import { Select } from '@kerfjs/ui/select';
import { delegate, delegateCapture, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { type ConversationActivity, type ConversationMessage, conversationUsage } from '../ai-conversation';
import { AIConversation } from '../components/ai-conversation';
import type { PermissionItem } from '../permission-notifications';
import { DEMO_ACTIONS, DEMO_COMPONENTS, DEMO_FIELDS } from './demo-actions';
import type { DemoModule, FormControl } from './demo-module';
import { sidebarEvent } from './project-sidebar-demo';
import { syncSettingsControls } from './settings-controls';

export type AIConversationScenario =
  'empty' | 'streaming' | 'permission' | 'completed' | 'usage-unpriced' | 'failed' | 'interrupted';
export const aiConversationScenario = signal<AIConversationScenario>('streaming');
export const aiConversationDemoOpen = signal(true);
export const aiConversationPresentation = signal<'dialog' | 'embedded'>('dialog');
export const aiConversationSaveCount = signal(0);
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
  { id: 'question-1', role: 'user', content: 'Review the client connection flow in `api.ts`.' },
  {
    id: 'answer-1',
    role: 'assistant',
    status: 'completed',
    content:
      'The connection uses the project event stream, so the transcript updates without simple polling.\n\n- Turns stay in one session.\n- Permissions appear inline.',
    usage: { tokensIn: 18_234, tokensOut: 2_101, costUsd: 0.0423, model: 'codex-5.6' },
  },
];
const activity: ConversationActivity[] = [
  {
    id: 'activity-1',
    tool: 'Codex',
    kind: 'edit',
    summary: 'Edited the conversation state boundary',
    importance: 'normal',
  },
  {
    id: 'activity-2',
    tool: 'Codex',
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
  };
}

export function AIConversationDemo() {
  const state = scenarioState();
  return (
    <section
      aria-label="AIConversation demo"
      class="ai-conversation-demo"
      data-presentation={aiConversationPresentation.value}
    >
      {aiConversationPresentation.value === 'dialog' && (
        <wa-button appearance="accent" data-action="open-ai-conversation-demo">
          Open conversation
        </wa-button>
      )}
      <output role="status">
        {aiConversationSaveCount.value
          ? `Prepared ${aiConversationSaveCount.value} conversation export${aiConversationSaveCount.value === 1 ? '' : 's'}.`
          : ''}
      </output>
      <AIConversation
        presentation={aiConversationPresentation.value}
        open={aiConversationDemoOpen.value}
        tool={aiConversationProviderLabel()}
        sessionId="019-demo-session"
        messages={state.messages}
        draft={aiConversationDraft.value}
        busy={state.busy}
        progress={state.progress}
        interruptible={state.interruptible}
        permissions={state.permissions}
        activity={state.activity}
        totalUsage={conversationUsage({ messages: state.messages })}
        error={state.error}
        feedbackAvailable
        providerId={aiConversationProvider.value}
        providers={AI_CONVERSATION_PROVIDERS}
        canChangeProvider
        model="gpt-5.6"
        effort="high"
        models={[
          { id: 'gpt-5.6', label: 'GPT-5.6' },
          { id: 'gpt-5.6-codex', label: 'GPT-5.6 Codex' },
        ]}
        efforts={['medium', 'high', 'xhigh']}
        canChangeModel
        canChangeEffort
      />
    </section>
  );
}

/** Restore the canonical conversation settings and sync the live controls (HS2-X1SM48). */
export function resetAIConversationDemo(root?: ParentNode): void {
  aiConversationPresentation.value = 'dialog';
  aiConversationScenario.value = 'streaming';
  aiConversationDemoOpen.value = true;
  if (root)
    syncSettingsControls(root, 'ai-conversation', {
      values: { presentation: aiConversationPresentation.value, scenario: aiConversationScenario.value },
    });
}

export function AIConversationSettings() {
  return (
    <form class="settings-form" data-settings="ai-conversation">
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
          { value: 'permission', label: 'Permission requested' },
          { value: 'completed', label: 'Completed with priced usage' },
          { value: 'usage-unpriced', label: 'Completed with unpriced usage' },
          { value: 'failed', label: 'Failed' },
          { value: 'interrupted', label: 'Interrupted' },
        ]}
      />
      {aiConversationPresentation.value === 'dialog' && (
        <wa-button type="button" data-action="open-ai-conversation-demo">
          Open conversation
        </wa-button>
      )}
      <wa-button type="button" data-action="reset-settings">
        Reset
      </wa-button>
    </form>
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
  demoListeners.add(
    delegate(root, 'change', '[data-settings="ai-conversation"] [name="presentation"]', (_event, target) => {
      aiConversationPresentation.value = (target as FormControl).value as typeof aiConversationPresentation.value;
      openAIConversationDemo(root);
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.saveConversation.selector, () => {
      aiConversationSaveCount.value++;
    }),
  );
  demoListeners.add(
    delegate(root, 'change', '[data-settings="ai-conversation"] [name="scenario"]', (_event, target) => {
      aiConversationScenario.value = (target as FormControl).value as typeof aiConversationScenario.value;
      openAIConversationDemo(root);
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.openAiConversationDemo.selector, () => {
      openAIConversationDemo(root);
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.closeConversation.selector, () => {
      root.querySelector<HTMLElement & { hide?(): void }>('[data-component="ai-conversation"]')?.hide?.();
      aiConversationDemoOpen.value = false;
    }),
  );
  demoListeners.add(
    delegateCapture(root, 'wa-hide', DEMO_COMPONENTS.aiConversation.selector, () => {
      aiConversationDemoOpen.value = false;
    }),
  );
  demoListeners.add(
    delegate(root, 'input', DEMO_FIELDS.conversationDraft.selector, (_event, target) => {
      aiConversationDraft.value = (target as HTMLTextAreaElement).value;
    }),
  );
  demoListeners.add(
    delegate(root, 'submit', DEMO_ACTIONS.sendConversationTurn.selector, (event) => {
      event.preventDefault();
      aiConversationScenario.value = 'streaming';
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.stopConversation.selector, () => {
      aiConversationScenario.value = 'interrupted';
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.selectConversationProvider.selector, (_event, target) => {
      const id = target.closest<HTMLElement>('[data-value]')?.dataset.value;
      if (id) {
        aiConversationProvider.value = id;
        sidebarEvent.value = `Switched conversation provider to ${aiConversationProviderLabel(id)}.`;
      }
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
