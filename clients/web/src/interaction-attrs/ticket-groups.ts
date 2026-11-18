import { attr } from 'kerfjs';
import { action } from 'kerfjs/actions';

/**
 * `data-action` specs for ticket groups (HS2-3XNTSD): the group header toggle, the group
 * name dialog, and the group header menu. Markup spreads `.attrs`; the delegated handlers
 * in `components/ticket-group-interactions.ts` register `.selector`.
 */
export const TICKET_GROUP_ACTIONS = {
  toggleTicketGroup: action('toggle-ticket-group'),
  /** The group header's trailing overflow button, which opens the group header menu (HS2-HKXSB1). */
  openTicketGroupMenu: action('open-ticket-group-menu'),
  submitTicketGroupName: action('submit-ticket-group-name'),
  cancelTicketGroupName: action('cancel-ticket-group-name'),
  /** Select every ticket a group shows in the list or column it was opened in (HS2-EYEGDQ). */
  selectTicketGroup: action('select-ticket-group'),
  renameTicketGroup: action('rename-ticket-group'),
  ungroupTicketGroup: action('ungroup-ticket-group'),
  mergeTicketGroups: action('merge-ticket-groups'),
} as const;

/**
 * Project Settings › Ticket groups (HS2-GXKX1Q): each row's View tickets and its "…" menu, the Merge
 * dialog, and the toolbar's New group. Menu items carry the group's id as `data-group-id`.
 */
export const TICKET_GROUP_SETTINGS_ACTIONS = {
  newSettingsGroup: action('new-settings-group'),
  viewGroupTickets: action('view-group-tickets'),
  renameSettingsGroup: action('rename-settings-group'),
  mergeSettingsGroup: action('merge-settings-group'),
  deleteSettingsGroup: action('delete-settings-group'),
  submitGroupMerge: action('submit-group-merge'),
  cancelGroupMerge: action('cancel-group-merge'),
} as const;

/** Other delegated targets for ticket groups. */
export const TICKET_GROUP_TARGETS = {
  group: attr('data-component', 'ticket-group'),
  groupNameField: attr('name', 'ticket-group-name'),
  groupNameDialog: attr('data-dialog-component', 'ticket-group-dialog'),
  groupMenu: attr('data-ticket-group-menu', 'true'),
  groupMergeDialog: attr('data-dialog-component', 'ticket-group-merge-dialog'),
  groupMergeInto: attr('name', 'ticket-group-merge-into'),
  /** A region where dropping a dragged ticket removes its group (HS2-PRXD39). */
  ungroupDropZone: attr('data-ticket-drop-ungroup', 'true'),
} as const;
