import { attr } from 'kerfjs';
import { action } from 'kerfjs/actions';

/**
 * `data-action` specs: terminal tiles, the terminal drawer, visibility groups, and project close.
 * Markup spreads `.attrs`; the delegated handlers in
 * `interactions/terminals.ts` register `.selector`.
 */
export const TERMINALS_ACTIONS = {
  exitTerminalFocusMode: action('exit-terminal-focus-mode'),
  zoomTerminalGrid: action('zoom-terminal-grid'),
  previewTerminal: action('preview-terminal'),
  openTerminalContextMenu: action('open-terminal-context-menu'),
  dismissMagnifiedTerminal: action('dismiss-magnified-terminal'),
  closeMagnifiedTerminal: action('close-magnified-terminal'),
  toggleTerminalModifier: action('toggle-terminal-modifier'),
  toggleTerminalFunctionRow: action('toggle-terminal-function-row'),
  sendTerminalKey: action('send-terminal-key'),
  cycleMobileTerminalColumns: action('cycle-mobile-terminal-columns'),
  hideDashboardTerminal: action('hide-dashboard-terminal'),
  openTerminalVisibility: action('open-terminal-visibility'),
  selectTerminalVisibilityTab: action('select-terminal-visibility-tab'),
  addTerminalVisibilityGroup: action('add-terminal-visibility-group'),
  renameTerminalVisibilityGroup: action('rename-terminal-visibility-group'),
  removeTerminalVisibilityGroup: action('remove-terminal-visibility-group'),
  submitTerminalVisibilityName: action('submit-terminal-visibility-name'),
  cancelTerminalVisibilityName: action('cancel-terminal-visibility-name'),
  toggleTerminalVisibility: action('toggle-terminal-visibility'),
  openTerminalProject: action('open-terminal-project'),
  openGridAiChat: action('open-grid-ai-chat'),
  toggleTerminalDrawer: action('toggle-terminal-drawer'),
  toggleTerminalDrawerMaximize: action('toggle-terminal-drawer-maximize'),
  selectDrawerItem: action('select-drawer-item'),
  createTerminalDrawerItem: action('create-terminal-drawer-item'),
  openSavedConversation: action('open-saved-conversation'),
  createProjectTerminal: action('create-project-terminal'),
  closeProjectTab: action('close-project-tab'),
  selectProjectCloseResource: action('select-project-close-resource'),
  cancelProjectClose: action('cancel-project-close'),
  confirmCloseProject: action('confirm-close-project'),
  closeAllProjectResources: action('close-all-project-resources'),
  closeTerminalTab: action('close-terminal-tab'),
  closeAiChatTab: action('close-ai-chat-tab'),
  renameTerminalForm: action('rename-terminal-form'),
  cancelTerminalRename: action('cancel-terminal-rename'),
} as const;

/**
 * Other delegated targets (components, named fields, flags) for
 * terminal tiles, the terminal drawer, visibility groups, and project close.
 * Markup spreads `.attrs` where it renders a literal; handlers use `.selector`.
 */
export const TERMINALS_TARGETS = {
  terminalTile: attr('data-component', 'terminal-tile'),
  terminalKeyBar: attr('data-component', 'terminal-key-bar'),
  terminalVisibilityGroupField: attr('name', 'terminal-visibility-group'),
  workspaceChatTile: attr('data-component', 'workspace-chat-tile'),
  projectCloseDialog: attr('data-component', 'project-close-dialog'),
} as const;
