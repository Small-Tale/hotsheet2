import { attr } from 'kerfjs';
import { action } from 'kerfjs/actions';

/**
 * `data-action` specs: commands, AI drive options, conversations, and command settings.
 * Markup spreads `.attrs`; the delegated handlers in
 * `interactions/commands-and-ai.ts` register `.selector`.
 */
export const COMMANDS_AND_AI_ACTIONS = {
  toggleCommandGroup: action('toggle-command-group'),
  toggleCommandSection: action('toggle-command-section'),
  toggleDrive: action('toggle-drive'),
  toggleDriveOptions: action('toggle-drive-options'),
  selectDriveDefault: action('select-drive-default'),
  selectDriveTool: action('select-drive-tool'),
  selectDriveModel: action('select-drive-model'),
  openDriveManualModel: action('open-drive-manual-model'),
  selectDriveEffort: action('select-drive-effort'),
  openConversation: action('open-conversation'),
  saveConversation: action('save-conversation'),
  pickConversationMessage: action('pick-conversation-message'),
  copyConversationSelection: action('copy-conversation-selection'),
  downloadConversationSelection: action('download-conversation-selection'),
  clearConversationSelection: action('clear-conversation-selection'),
  toggleConversationActivity: action('toggle-conversation-activity'),
  submitConversationExport: action('submit-conversation-export'),
  sendConversationTurn: action('send-conversation-turn'),
  stopConversation: action('stop-conversation'),
  selectConversationProvider: action('select-conversation-provider'),
  selectConversationModel: action('select-conversation-model'),
  selectConversationEffort: action('select-conversation-effort'),
  selectConversationPermissionMode: action('select-conversation-permission-mode'),
  openConversationManualModel: action('open-conversation-manual-model'),
  rateAiContent: action('rate-ai-content'),
  submitAiFeedback: action('submit-ai-feedback'),
  cancelAiFeedback: action('cancel-ai-feedback'),
  runCommand: action('run-command'),
  dismissCommandDialog: action('dismiss-command-dialog'),
  confirmStopCommand: action('confirm-stop-command'),
  editCommandSetting: action('edit-command-setting'),
  selectCommandSetting: action('select-command-setting'),
  addCommandSetting: action('add-command-setting'),
  closeCommandEditor: action('close-command-editor'),
  deleteCommandSetting: action('delete-command-setting'),
  addCommandGroup: action('add-command-group'),
  deleteCommandGroup: action('delete-command-group'),
  selectCommandIcon: action('select-command-icon'),
  selectCommandColor: action('select-command-color'),
  selectCommandAiDefault: action('select-command-ai-default'),
  selectCommandAiTool: action('select-command-ai-tool'),
  selectCommandAiModel: action('select-command-ai-model'),
  selectCommandAiEffort: action('select-command-ai-effort'),
  openCommandManualModel: action('open-command-manual-model'),
  toggleLoadingActivity: action('toggle-loading-activity'),
  toggleGlobalShellHistory: action('toggle-global-shell-history'),
  saveTerminalLimits: action('save-terminal-limits'),
  saveTrashSettings: action('save-trash-settings'),
  revertTrashSettings: action('revert-trash-settings'),
  applyLeavingTrashSettings: action('apply-leaving-trash-settings'),
  setViewMode: action('set-view-mode'),
  selectSettingsCategory: action('select-settings-category'),
  editShortcut: action('edit-shortcut'),
  cancelShortcutCapture: action('cancel-shortcut-capture'),
  resetShortcut: action('reset-shortcut'),
  resetAllShortcuts: action('reset-all-shortcuts'),
  cancelManualModel: action('cancel-manual-model'),
  submitManualModel: action('submit-manual-model'),
  openProviderDialog: action('open-provider-dialog'),
  editProviderConnection: action('edit-provider-connection'),
  removeProjectSource: action('remove-project-source'),
  useGithubAccount: action('use-github-account'),
  useProviderAccount: action('use-provider-account'),
  signOutAccount: action('sign-out-account'),
  requestUnusedAccountSourceRemoval: action('request-unused-account-source-removal'),
  cancelUnusedAccountSourceRemoval: action('cancel-unused-account-source-removal'),
  removeUnusedAccountSource: action('remove-unused-account-source'),
  requestProviderRemoval: action('request-provider-removal'),
  cancelProviderRemoval: action('cancel-provider-removal'),
  confirmProviderRemoval: action('confirm-provider-removal'),
  toggleProviderDisabled: action('toggle-provider-disabled'),
  refreshGithubRepositories: action('refresh-github-repositories'),
  chooseGithubEnterprise: action('choose-github-enterprise'),
  chooseGithubDotcom: action('choose-github-dotcom'),
  copyGithubCode: action('copy-github-code'),
  reopenGithubSignIn: action('reopen-github-sign-in'),
  selectProviderKind: action('select-provider-kind'),
  backProviderKind: action('back-provider-kind'),
  startGithubSignIn: action('start-github-sign-in'),
  cancelGithubSignIn: action('cancel-github-sign-in'),
  submitProviderSetup: action('submit-provider-setup'),
  saveProviderConnection: action('save-provider-connection'),
} as const;

/**
 * Other delegated targets (components, named fields, flags) for
 * commands, AI drive options, conversations, and command settings.
 * Markup spreads `.attrs` where it renders a literal; handlers use `.selector`.
 */
export const COMMANDS_AND_AI_TARGETS = {
  aiConversation: attr('data-component', 'ai-conversation'),
  /** The dialog presentation's root, a Kerf DialogSurface whose own `data-component` is protected. */
  aiConversationDialog: attr('data-dialog-component', 'ai-conversation'),
  conversationExportDialog: attr('data-dialog-component', 'conversation-export-dialog'),
  conversationDraftField: attr('name', 'conversation-draft'),
  commandIconSearchField: attr('name', 'command-icon-search'),
  manualModelDialog: attr('data-dialog-component', 'manual-model-dialog'),
  aiFeedbackDialog: attr('data-dialog-component', 'ai-feedback-dialog'),
  trashRetentionDaysField: attr('name', 'trash-cleanup-days'),
  trashRetentionLeaveDialog: attr('data-dialog-component', 'trash-retention-leave-dialog'),
} as const;

/**
 * The edit-command fields whose `input` events autosave: Kerf TextField roots mark their native
 * inputs with `data-command-field`, the Kerf Type Select is addressed by its name (HS2-WNXRG3), and the Kerf
 * ChoiceChips color radios by their action (HS2-EBY4E3).
 */
export const COMMAND_FIELD_SELECTOR =
  '[data-command-field] input, [data-command-field] textarea, [data-command-editor-dialog] kui-select[name="kind"], [data-action="select-command-color"]';
