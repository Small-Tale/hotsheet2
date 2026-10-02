import { attr } from 'kerfjs';
import { action } from 'kerfjs/actions';

/**
 * `data-action` specs: project tabs, workspace navigation, and corrupt-ticket rows.
 * Markup spreads `.attrs`; the delegated handlers in
 * `interactions/navigation-and-tabs.ts` register `.selector`.
 */
export const NAVIGATION_AND_TABS_ACTIONS = {
  revealCorruptTicket: action('reveal-corrupt-ticket'),
  repairCorruptTicket: action('repair-corrupt-ticket'),
  selectCorruptTicket: action('select-corrupt-ticket'),
  setShellMode: action('set-shell-mode'),
  openProjectStats: action('open-project-stats'),
  selectProjectTab: action('select-project-tab'),
  retryProjectRestore: action('retry-project-restore'),
} as const;

/**
 * Other delegated targets (components, named fields, flags) for
 * project tabs, workspace navigation, and corrupt-ticket rows.
 * Markup spreads `.attrs` where it renders a literal; handlers use `.selector`.
 */
export const NAVIGATION_AND_TABS_TARGETS = {
  tabRole: attr('role', 'tab'),
} as const;
