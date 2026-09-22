import type { Signal } from 'kerfjs';
import { signal } from 'kerfjs';

import { type ConversationMessage, type ConversationState, EMPTY_CONVERSATION } from '../ai-conversation';
import { Api, type ToolConnection } from '../api';
import { type ConversationExportDialogState } from '../components/conversation-export-dialog';
import {
  buildConversationExportRequest,
  conversationExportAssets,
  type ConversationExportDestination,
  type ConversationExportDraft,
  type ConversationExportOpenResult,
  type ConversationExportScope,
  conversationExportScopeAfterMessagePick,
  type ConversationExportWriteResult,
  conversationTranscriptMarkdown,
  defaultConversationExportDraft,
  selectedConversationMessages,
  suggestedConversationExportName,
} from '../conversation-export';
import type { Project } from '../interactions/types';
import { type DrawerAIChat } from '../project-drive';
import { hideNewTerminalInNamedGroups, type TerminalVisibilityState } from '../terminal-visibility';

export interface ConversationArchiveDependencies {
  project: () => Project | undefined;
  conversationConnectionId: Signal<string | undefined>;
  conversationStates: Signal<Record<string, ConversationState>>;
  driveConnectionsByProject: Signal<Record<string, ToolConnection[]>>;
  terminalDrawerChatsByProject: Signal<Record<string, DrawerAIChat[]>>;
  conversationAiSelection: (connectionId: string) => { tool: string; model?: string; effort?: string };
  aiToolLabel: (tool: string) => string;
  showToast: (message: string) => void;
  error: Signal<string>;
  terminalDrawerCreateMenuOpen: Signal<boolean>;
  terminalVisibility: Signal<TerminalVisibilityState>;
  persistTerminalVisibility: (next: TerminalVisibilityState) => void;
  replaceConversationStates: (states: Record<string, ConversationState>) => void;
  selectDrawerItem: (id: string) => void;
  setTerminalDrawerVisible: (visible: boolean, refresh?: boolean) => void;
}

export function createConversationArchiveController(dependencies: ConversationArchiveDependencies) {
  const {
    project,
    conversationConnectionId,
    conversationStates,
    driveConnectionsByProject,
    terminalDrawerChatsByProject,
    conversationAiSelection,
    aiToolLabel,
    showToast,
    error,
    terminalDrawerCreateMenuOpen,
    terminalVisibility,
    persistTerminalVisibility,
    replaceConversationStates,
    selectDrawerItem,
    setTerminalDrawerVisible,
  } = dependencies;
  const conversationSelectionScopes = signal<Record<string, ConversationExportScope | undefined>>({});
  const conversationExportDialog = signal<ConversationExportDialogState | undefined>(undefined);

  function conversationSelectedMessages(connectionId: string, messages: readonly ConversationMessage[]) {
    const scope = conversationSelectionScopes.value[connectionId];
    return scope?.kind === 'range' ? [...selectedConversationMessages(messages, scope)] : [];
  }

  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  function pickConversationMessage(connectionId:string,messageId:string){const messages=conversationStates.peek()[connectionId]?.messages??[],scope=conversationSelectionScopes.value[connectionId]??{kind:'all' as const},next=conversationExportScopeAfterMessagePick(messages,scope,messageId);conversationSelectionScopes.value={...conversationSelectionScopes.value,[connectionId]:next}}

  function clearConversationSelection(connectionId: string) {
    conversationSelectionScopes.value = Object.fromEntries(
      Object.entries(conversationSelectionScopes.value).filter(([id]) => id !== connectionId),
    );
  }

  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  async function copyConversationSelection(connectionId:string){const messages=conversationSelectedMessages(connectionId,conversationStates.peek()[connectionId]?.messages??[]);if(!messages.length)return;const tool=aiToolLabel(conversationAiSelection(connectionId).tool);try{await navigator.clipboard.writeText(conversationTranscriptMarkdown({conversationId:connectionId,tool},messages));showToast(`${messages.length} selected message${messages.length===1?'':'s'} copied to clipboard.`)}catch(reason){error.value=`Copy failed: ${reason instanceof Error?reason.message:String(reason)}`}}

  function updateConversationExportDraft(update: (draft: ConversationExportDraft) => ConversationExportDraft) {
    const state = conversationExportDialog.value;
    if (state) conversationExportDialog.value = { ...state, draft: update(state.draft), error: '' };
  }

  function openConversationExport() {
    const current = project(),
      connectionId = conversationConnectionId.value;
    if (!current || !connectionId) return;
    const state = conversationStates.peek()[connectionId] ?? EMPTY_CONVERSATION;
    if (!state.messages.length) return;
    const connection = (driveConnectionsByProject.value[current.id] ?? []).find((item) => item.id === connectionId),
      chat = (terminalDrawerChatsByProject.value[current.id] ?? []).find((item) => item.connectionId === connectionId),
      selection = conversationAiSelection(connectionId),
      tool = chat?.tool ?? connection?.tool ?? selection.tool,
      scope = conversationSelectionScopes.value[connectionId],
      selectedRange =
        scope?.kind === 'range' && selectedConversationMessages(state.messages, scope).length ? scope : undefined,
      draft = { ...defaultConversationExportDraft(), ...(selectedRange ? { scope: selectedRange } : {}) };
    conversationExportDialog.value = {
      source: {
        conversationId: chat?.sourceConversationId ?? connectionId,
        tool,
        projectId: current.id,
        sessionId: chat?.sourceSessionId ?? connection?.session_id,
        model: selection.model,
        effort: selection.effort,
        resumable: !chat?.readOnly,
      },
      messages: [...state.messages],
      activity: [...(state.activity ?? [])],
      draft,
      summaryAvailable: true,
      step: selectedRange ? 1 : 2,
      navigation: 'none',
      selectedRange,
    };
  }

  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  async function pickConversationExportDestination(){const state=conversationExportDialog.value;if(!state||state.busy)return;conversationExportDialog.value={...state,busy:true,error:''};try{const response=await fetch('/__hotsheet/conversation-exports/destination',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({suggestedName:suggestedConversationExportName(state.source.tool)})}),result=await response.json() as {destination?:ConversationExportDestination;error?:string};if(!response.ok)throw new Error(result.error??'Could not choose a conversation export destination.');if(conversationExportDialog.value){if(!result.destination){conversationExportDialog.value={...state,busy:false};return}conversationExportDialog.value={...state,busy:false,draft:{...state.draft,destination:result.destination,writeMode:'create'},error:''}}}catch(reason){if(conversationExportDialog.value)conversationExportDialog.value={...state,busy:false,error:reason instanceof Error?reason.message:String(reason)}}}

  async function saveConversationExport() {
    const state = conversationExportDialog.value;
    if (!state || state.busy) return;
    try {
      const request = buildConversationExportRequest(state.source, state.messages, state.draft),
        messages = [...selectedConversationMessages(state.messages, state.draft.scope)];
      conversationExportDialog.value = { ...state, busy: true, error: '' };
      const assets = await conversationExportAssets(state.messages, state.draft),
        response = await fetch('/__hotsheet/conversation-exports/write', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ request, messages, activity: state.activity ?? [], assets }),
        }),
        result = (await response.json()) as ConversationExportWriteResult & { error?: string };
      if (!response.ok) throw new Error(result.error ?? 'Could not save the conversation.');
      conversationExportDialog.value = undefined;
      showToast(`Saved conversation revision ${result.manifest.revision}.`);
    } catch (reason) {
      if (conversationExportDialog.value)
        conversationExportDialog.value = {
          ...conversationExportDialog.value,
          busy: false,
          error: reason instanceof Error ? reason.message : String(reason),
        };
    }
  }

  async function finishConversationExport() {
    const state = conversationExportDialog.value;
    if (!state || state.busy) return;
    if (!state.draft.destination) {
      await pickConversationExportDestination();
      const selected = conversationExportDialog.value;
      if (!selected?.draft.destination || selected.draft.destination.existing) return;
    }
    await saveConversationExport();
  }

  async function openSavedConversation() {
    const current = project();
    if (!current) return;
    terminalDrawerCreateMenuOpen.value = false;
    try {
      const response = await fetch('/__hotsheet/conversation-exports/open', { method: 'POST' }),
        result = (await response.json()) as { conversation?: ConversationExportOpenResult; error?: string };
      if (!response.ok) throw new Error(result.error ?? 'Could not open the saved conversation.');
      if (!result.conversation) return;
      const saved = result.conversation,
        source = saved.manifest.source,
        reopen = saved.manifest.reopen,
        canResume = Boolean(
          reopen.resumesOriginalSession && reopen.sessionId && (!source.projectId || source.projectId === current.id),
        );
      let connectionId = `hotsheet-saved-chat-${crypto.randomUUID()}`,
        readOnly = true,
        localOnly = true,
        resumeError = '';
      if (canResume) {
        try {
          const created = await new Api(current.apiPath).createToolConnection({
            tool: source.tool,
            checkout: current.id,
            connection_id: connectionId,
            session_id: reopen.sessionId,
            model: source.model,
            effort: source.effort,
          });
          connectionId = created.id;
          localOnly = false;
          driveConnectionsByProject.value = {
            ...driveConnectionsByProject.value,
            [current.id]: (driveConnectionsByProject.value[current.id] ?? [])
              .filter((item) => item.id !== created.id)
              .concat(created),
          };
          readOnly = !created.actions?.includes('send_turn');
        } catch (reason) {
          resumeError = reason instanceof Error ? reason.message : String(reason);
        }
      }
      const tab: DrawerAIChat = {
        id: `ai-chat:${connectionId}`,
        connectionId,
        tool: source.tool,
        name: `${aiToolLabel(source.tool)} saved chat`,
        model: source.model,
        effort: source.effort,
        readOnly,
        localOnly,
        savedSource: saved.displayPath,
        sourceConversationId: source.conversationId,
        sourceSessionId: source.sessionId,
      };
      terminalDrawerChatsByProject.value = {
        ...terminalDrawerChatsByProject.value,
        [current.id]: [...(terminalDrawerChatsByProject.value[current.id] ?? []), tab],
      };
      persistTerminalVisibility(hideNewTerminalInNamedGroups(terminalVisibility.value, `${current.id}:${tab.id}`));
      replaceConversationStates({
        ...conversationStates.peek(),
        [connectionId]: { messages: saved.messages, activity: saved.activity },
      });
      selectDrawerItem(tab.id);
      setTerminalDrawerVisible(true);
      showToast(
        readOnly
          ? resumeError
            ? `Opened read-only; resume failed: ${resumeError}`
            : 'Opened saved conversation read-only.'
          : 'Opened saved conversation; you can continue it.',
      );
    } catch (reason) {
      error.value = reason instanceof Error ? reason.message : String(reason);
    }
  }

  return {
    conversationExportDialog,
    conversationSelectedMessages,
    pickConversationMessage,
    clearConversationSelection,
    copyConversationSelection,
    updateConversationExportDraft,
    openConversationExport,
    finishConversationExport,
    openSavedConversation,
  };
}
