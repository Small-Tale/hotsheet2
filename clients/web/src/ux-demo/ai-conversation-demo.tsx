import { signal } from 'kerfjs';

import { type ConversationActivity, type ConversationMessage, conversationUsage } from '../ai-conversation';
import { AIConversation } from '../components/ai-conversation';
import type { PermissionItem } from '../permission-notifications';

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
  { id: 'question-1', role: 'user', content: 'Review the client connection flow.' },
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

export function AIConversationSettings() {
  return (
    <form class="settings-form" data-settings="ai-conversation">
      <wa-select name="presentation" label="Presentation" value={aiConversationPresentation.value}>
        <wa-option value="dialog">Dialog</wa-option>
        <wa-option value="embedded">Embedded</wa-option>
      </wa-select>
      <wa-select name="scenario" label="Public state" value={aiConversationScenario.value}>
        <wa-option value="empty">Empty</wa-option>
        <wa-option value="streaming">Streaming</wa-option>
        <wa-option value="permission">Permission requested</wa-option>
        <wa-option value="completed">Completed with priced usage</wa-option>
        <wa-option value="usage-unpriced">Completed with unpriced usage</wa-option>
        <wa-option value="failed">Failed</wa-option>
        <wa-option value="interrupted">Interrupted</wa-option>
      </wa-select>
      {aiConversationPresentation.value === 'dialog' && (
        <wa-button type="button" data-action="open-ai-conversation-demo">
          Open conversation
        </wa-button>
      )}
    </form>
  );
}
