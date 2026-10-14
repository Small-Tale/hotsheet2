import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import '@awesome.me/webawesome/dist/components/option/option.js';
import '@awesome.me/webawesome/dist/styles/utilities/visually-hidden.css';
import './quick-ticket-composer.css';

import { foregroundColorVar } from '@kerfjs/ui/css-values';
import { FileDropZone, FilePickerTrigger } from '@kerfjs/ui/file-picker';
import { IconButton } from '@kerfjs/ui/icon-button';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Select } from '@kerfjs/ui/select';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text } from '@kerfjs/ui/text';
import { Paperclip, Plus, Star, Trash2, TriangleAlert, Upload } from 'lucide';

import { SEARCH_AND_COMPOSER_ACTIONS, SEARCH_AND_COMPOSER_TARGETS } from '../interaction-attrs/search-and-composer';
import { SHELL_AND_GLOBAL_TARGETS } from '../interaction-attrs/shell-and-global';
import { TicketCategorySelect } from './ticket-category-select';

/** The warning foreground of the composer's attachment notices. */
const WARNING_TEXT = foregroundColorVar('--wa-color-warning-on-quiet');
export interface QuickTicketComposerProps {
  expanded?: boolean;
  title?: string;
  details?: string;
  category?: string;
  upNext?: boolean;
  providerName?: string;
  /** The project default, which may differ from the selected or only writable source. */
  defaultSourceName?: string;
  sourceIsDefault?: boolean;
  /**
   * Writable ticket sources the new ticket can target. With two or more the footer offers a
   * source Select instead of the plain "Creating in" text (HS2-NZMJBJ).
   */
  sources?: readonly { value: string; label: string }[];
  /** The selected source's connection id. */
  source?: string;
  canCreate?: boolean;
  attachments?: readonly { id: string; name: string }[];
  attachmentsEnabled?: boolean;
  attachmentMessage?: string;
  attachmentError?: boolean;
  busy?: boolean;
  submitting?: boolean;
}

export interface QuickTicketDialogElement extends HTMLElement {
  open: boolean;
  show(): Promise<void>;
}

/**
 * Whether staged new-ticket files are stranded: the selected source cannot take attachments, so
 * creation is blocked until the user removes them or picks a source that can (HS2-8HHHK3).
 */
/** The error a blocked submit reports; a later source switch clears it (HS2-8HHHK3). */
export const STRANDED_ATTACHMENTS_MESSAGE =
  'Remove the staged attachments or choose a ticket source that supports attachments.';

/** `wireFilePickers` id of the composer header's Add trigger. */
export const NEW_TICKET_ATTACHMENT_PICKER = 'new-ticket-attachments';

export function strandedNewTicketAttachments(attachmentCount: number, attachmentsEnabled: boolean): boolean {
  return !attachmentsEnabled && attachmentCount > 0;
}

export function showQuickTicketComposer(root: ParentNode): boolean {
  const dialog = root.querySelector<QuickTicketDialogElement>('[data-component="quick-ticket-composer"]');
  if (!dialog) return false;
  if (dialog.childElementCount === 0) {
    requestAnimationFrame(() => showQuickTicketComposer(root));
    return false;
  }
  const nativeDialog = dialog.shadowRoot?.querySelector('dialog');
  if (nativeDialog) nativeDialog.setAttribute('role', 'presentation');
  if (dialog.open && (!nativeDialog || nativeDialog.open)) return false;
  void dialog.show();
  return true;
}

export function focusQuickTicketComposerTitle(root: ParentNode): boolean {
  const input = root.querySelector<HTMLElement>('[name="new-ticket-title"]');
  if (!input) return false;
  input.focus({ preventScroll: true });
  return true;
}

/**
 * The pill that opens the quick ticket composer and accepts dropped tickets or files. `size`
 * `compact` is the shorter trigger a narrow heading toolbar uses, such as the ticket rail's
 * (HS2-8FS5BJ); `default` is the workspace tab bar's.
 */
export function QuickTicketLauncher({
  attachmentsEnabled = true,
  label = 'New ticket…',
  size = 'default',
}: { attachmentsEnabled?: boolean; label?: string; size?: 'default' | 'compact' } = {}) {
  // A brand Web Awesome pill: Kerf's TabBar `end` zone accepts one standalone `wa-button`, and the
  // composition extension declares this launcher `rendersAs @kerfjs/ui:wa-button` (HS2-PNCDAE).
  return (
    <wa-button
      variant="brand"
      pill
      size={size === 'compact' ? 'small' : 'medium'}
      class="quick-ticket-composer__launcher"
      data-size={size}
      data-component="quick-ticket-composer-launcher"
      {...SEARCH_AND_COMPOSER_ACTIONS.expandTicketComposer.attrs}
      {...SEARCH_AND_COMPOSER_TARGETS.newTicketDropTarget.attrs}
      {...SHELL_AND_GLOBAL_TARGETS.duplicateTicketDropAction.attrs}
      title={
        attachmentsEnabled
          ? 'Create a new ticket, drop tickets to duplicate, or drop attachment files here'
          : 'Create a new ticket or drop tickets to duplicate'
      }
    >
      <LucideIcon size="s" slot="start" icon={Plus} name="plus" />
      {label}
    </wa-button>
  );
}

export function QuickTicketComposer({
  expanded = false,
  title = '',
  details = '',
  category = 'task',
  upNext = false,
  providerName = 'Hot Sheet',
  defaultSourceName = providerName,
  sourceIsDefault = true,
  sources = [],
  source,
  canCreate = true,
  attachments = [],
  attachmentsEnabled = true,
  attachmentMessage = '',
  attachmentError = false,
  busy = false,
  submitting = false,
}: QuickTicketComposerProps) {
  // Files staged for a source that takes attachments stay listed after switching to one that does
  // not; creation is blocked until the user removes them or switches back (HS2-8HHHK3).
  const strandedAttachments = strandedNewTicketAttachments(attachments.length, attachmentsEnabled);
  return (
    <wa-dialog
      class="quick-ticket-dialog"
      {...SEARCH_AND_COMPOSER_TARGETS.quickTicketComposer.attrs}
      data-key="quick-ticket-composer"
      label="Create ticket"
      role="dialog"
      aria-modal={expanded ? 'true' : 'false'}
      aria-label="Create ticket"
      // Kerf 5.0.0-beta.56 leaves a custom element's role/aria-* alone when the template omits it, so
      // every state renders an explicit value rather than relying on the morph to remove it.
      aria-hidden={expanded ? 'false' : 'true'}
      inert={expanded ? undefined : true}
      open={expanded || undefined}
      data-controlled-open={String(expanded)}
    >
      {expanded && (
        <form
          class="quick-ticket-composer"
          {...SEARCH_AND_COMPOSER_ACTIONS.createTicketForm.attrs}
          {...SEARCH_AND_COMPOSER_TARGETS.newTicketDropTarget.attrs}
          {...SHELL_AND_GLOBAL_TARGETS.duplicateTicketDropAction.attrs}
          data-submitting={String(submitting)}
        >
          {!sourceIsDefault && (
            <div class="quick-ticket-composer__source-warning">
              <StateBanner
                title={`Creating in ${providerName}`}
                detail={`This is not the default ticket source (${defaultSourceName}).`}
                tone="warning"
                urgency="status"
                copyLayout="stacked"
                icon={<LucideIcon icon={TriangleAlert} name="triangle-alert" />}
              />
            </div>
          )}
          <wa-input
            class="quick-ticket-composer__title"
            name="new-ticket-title"
            label="Ticket Title"
            value={title}
            autofocus
            required
          />
          <div class="quick-ticket-composer__metadata">
            <TicketCategorySelect name="new-ticket-category" label="Category" value={category} />
            <button
              type="button"
              class="quick-ticket-composer__up-next"
              {...SEARCH_AND_COMPOSER_ACTIONS.toggleNewTicketUpNext.attrs}
              aria-pressed={String(upNext)}
              aria-label={upNext ? 'Remove new ticket from Up Next' : 'Add new ticket to Up Next'}
              title={upNext ? 'Remove from Up Next' : 'Add to Up Next'}
            >
              <LucideIcon size="s" icon={Star} name="star" appearance={upNext ? 'solid' : 'outline'} />
            </button>
          </div>
          <label class="quick-ticket-composer__details">
            <span>Details</span>
            <textarea name="new-ticket-details" rows={1} data-morph-skip>
              {details}
            </textarea>
          </label>
          <section class="quick-ticket-composer__attachments" aria-label="New Ticket Attachments">
            <header>
              <span>
                <LucideIcon size={14.4} icon={Paperclip} name="paperclip" />
                Attachments
              </span>
              {attachmentsEnabled && (
                <FilePickerTrigger
                  label="Add"
                  accessibleLabel="Browse attachments for new ticket"
                  icon={<LucideIcon icon={Plus} name="plus" />}
                  multiple
                  name="new-ticket-attachments"
                  pickerId={NEW_TICKET_ATTACHMENT_PICKER}
                />
              )}
              {strandedAttachments && (
                <wa-button
                  appearance="plain"
                  variant="danger"
                  size="small"
                  {...SEARCH_AND_COMPOSER_ACTIONS.clearNewTicketAttachments.attrs}
                  disabled={submitting}
                >
                  <LucideIcon slot="start" size={14.4} icon={Trash2} name="trash-2" />
                  {/* wa-button names its inner button from slotted text, not the host aria-label (HS2-6TMT6Q). */}
                  Remove all<span class="wa-visually-hidden"> staged attachments</span>
                </wa-button>
              )}
            </header>
            {attachments.length > 0 && (
              <div class="quick-ticket-composer__attachment-list">
                {attachments.map((item) => (
                  <div class="quick-ticket-composer__attachment" data-pending-attachment-id={item.id}>
                    <LucideIcon size={14.4} icon={Paperclip} name="paperclip" />
                    <span title={item.name}>{item.name}</span>
                    <IconButton
                      icon={Trash2}
                      iconName="trash-2"
                      label={`Remove ${item.name}`}
                      action={SEARCH_AND_COMPOSER_ACTIONS.removeNewTicketAttachment.value}
                      appearance="plain"
                      variant="danger"
                      size="xs"
                      rootAttributes={{ 'data-pending-attachment-id': item.id }}
                    />
                  </div>
                ))}
              </div>
            )}
            {attachmentsEnabled ? (
              <FileDropZone
                title="Drop attachment files anywhere in this area or browse"
                accessibleLabel="Drop or browse attachments for new ticket"
                icon={<LucideIcon icon={Upload} name="upload" />}
                multiple
                name="new-ticket-attachments"
                pickerId={NEW_TICKET_ATTACHMENT_PICKER}
              />
            ) : strandedAttachments ? (
              <Text
                variant="p"
                size="compact"
                flush
                color={WARNING_TEXT}
                role="status"
                data-new-ticket-attachments-stranded="true"
              >
                {providerName} does not support attachments. Remove the{' '}
                {attachments.length === 1 ? 'staged file' : `${attachments.length} staged files`} or choose a source
                that supports attachments to create this ticket.
              </Text>
            ) : (
              <Text variant="p" size="compact" flush color={WARNING_TEXT}>
                This ticket provider does not support attachments.
              </Text>
            )}
            {attachmentMessage && (
              <Text
                variant="p"
                size="compact"
                flush
                tone={attachmentError ? 'danger' : 'quiet'}
                role={attachmentError ? 'alert' : 'status'}
              >
                {attachmentMessage}
              </Text>
            )}
          </section>
          <div class="quick-ticket-composer__footer">
            {sources.length > 1 ? (
              <Row gap="xs" vAlign="middle" className="quick-ticket-composer__source">
                <Text variant="span" tone="quiet" size="compact" border="none" flush aria-hidden="true">
                  Creating in
                </Text>
                <Select
                  name="new-ticket-source"
                  ariaLabel="Ticket source"
                  value={source ?? sources[0].value}
                  choices={sources}
                  size="compact"
                  triggerWidth="fit-content"
                  disabled={submitting}
                />
              </Row>
            ) : (
              <Text variant="span" tone="quiet" size="compact" border="none" flush>
                Creating in {providerName}
              </Text>
            )}
            <div class="quick-ticket-composer__actions">
              <wa-button
                type="button"
                appearance="plain"
                data-dialog="close"
                data-action="cancel-ticket-composer"
                disabled={submitting}
              >
                Cancel
              </wa-button>
              <wa-button
                type="submit"
                appearance="accent"
                disabled={!canCreate || strandedAttachments || busy || submitting}
              >
                {submitting ? 'Creating…' : 'Create ticket'}
              </wa-button>
            </div>
          </div>
          {!canCreate && (
            <p class="quick-ticket-composer__notice" role="status">
              This ticket provider does not support creating tickets.
            </p>
          )}
        </form>
      )}
    </wa-dialog>
  );
}
