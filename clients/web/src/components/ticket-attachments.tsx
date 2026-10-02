import './ticket-inspector-panel.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { MoreHorizontal, Paperclip, Pencil, Plus, Upload } from 'lucide';

import type { AttachmentActor, AttachmentPurpose } from '../api';
import { isGalleryMediaAttachment, isVideoAttachment } from '../attachment-references';

export interface TicketAttachmentItem {
  id: string;
  name: string;
  url?: string;
  thumbnailUrl?: string;
  manageVideoPoster?: boolean;
  annotationCount?: number;
  round?: number;
  batch_id?: string;
  batch_label?: string;
  actor?: AttachmentActor;
  purpose?: AttachmentPurpose;
}
export interface AttachmentBatch {
  key: string;
  id?: string;
  label: string;
  explicitLabel?: string;
  actor?: AttachmentActor;
  purpose?: AttachmentPurpose;
  items: TicketAttachmentItem[];
}
export const DEFAULT_ATTACHMENTS: readonly TicketAttachmentItem[] = [
  { id: 'wireframe', name: 'wireframe.png' },
  { id: 'requirements', name: 'requirements.md' },
];

const purposeLabel = (purpose?: AttachmentPurpose) =>
  purpose
    ? (
        {
          problem_evidence: 'Problem evidence',
          correctness_evidence: 'Correctness evidence',
          reference: 'Reference',
          other: 'Other',
        } as const
      )[purpose]
    : '';

/** Deterministic grouping. Missing provider/legacy metadata is never guessed. */
export function groupAttachments(attachments: readonly TicketAttachmentItem[]): AttachmentBatch[] {
  const groups = new Map<string, AttachmentBatch>(),
    rounds = new Map<string, number>();
  for (const item of attachments) {
    const role = item.actor?.role ?? 'legacy',
      round = item.round ?? 1,
      key = item.batch_id
        ? item.batch_label
          ? item.batch_id
          : `__round__:${role}:${round}:${item.purpose ?? ''}`
        : '__legacy__';
    let group = groups.get(key);
    if (!group) {
      const fallbackRound = (rounds.get(role) ?? 0) + 1;
      rounds.set(role, fallbackRound);
      const shownRound = item.round ?? fallbackRound,
        who =
          item.actor?.display_name ??
          (role === 'ai'
            ? 'AI'
            : role === 'human'
              ? 'Human'
              : role === 'system'
                ? 'System'
                : role === 'unknown'
                  ? 'Unknown'
                  : 'Legacy / Uncategorized'),
        derived = item.batch_id
          ? `${who} · ${role === 'ai' || role === 'human' ? 'Round' : 'Batch'} ${shownRound}`
          : 'Legacy / Uncategorized',
        purpose = purposeLabel(item.purpose);
      group = {
        key,
        id: item.batch_id,
        label: item.batch_label ?? `${derived}${purpose ? ` · ${purpose}` : ''}`,
        explicitLabel: item.batch_label,
        actor: item.actor,
        purpose: item.purpose,
        items: [],
      };
      groups.set(key, group);
    }
    group.items.push(item);
  }
  return [...groups.values()];
}

export function TicketAttachments({
  attachments = DEFAULT_ATTACHMENTS,
  enabled = true,
  editable = enabled,
  message = '',
}: {
  attachments?: readonly TicketAttachmentItem[];
  /** New files can be added (the provider reports `attachments`). */
  enabled?: boolean;
  /**
   * Existing attachments can be regrouped, re-labelled, renamed, deleted, and opened locally
   * (the provider reports `attachment_edit`). An append-only provider such as GitHub's assets
   * repository (HS2-HSA64D) adds files but shows existing ones read-only, opened by link.
   */
  editable?: boolean;
  message?: string;
}) {
  const manageable = enabled && editable;
  const groups = groupAttachments(attachments);
  return (
    <div
      class="ticket-inspector__content ticket-attachments"
      data-component="ticket-attachments"
      data-attachment-drop-target={String(enabled)}
    >
      <section>
        <header class="ticket-inspector__section-header">
          <h2>
            Attachments{' '}
            <span
              class="ticket-attachments__count"
              aria-label={`${attachments.length} ${attachments.length === 1 ? 'attachment' : 'attachments'}`}
            >
              {attachments.length}
            </span>
          </h2>
          {enabled && (
            <label class="ticket-attachments__browse">
              <LucideIcon icon={Plus} name="plus" />
              <span>Add</span>
              <input type="file" name="ticket-attachments" multiple aria-label="Browse and add attachments" />
            </label>
          )}
        </header>
        <div class="ticket-attachments__batches">
          {groups.map((group) => {
            const media = group.items.filter((attachment): attachment is TicketAttachmentItem & { url: string } =>
                Boolean(attachment.url && isGalleryMediaAttachment(attachment.name)),
              ),
              shownLabel = group.explicitLabel ?? group.label;
            return (
              <section
                class="ticket-attachments__batch"
                // Regroup targets exist only where attachments are editable (HS2-0RTH3J).
                data-attachment-group-drop-target={manageable ? 'true' : undefined}
                data-attachment-batch={group.id ?? ''}
                data-attachment-ids={group.items.map((item) => item.id).join(',')}
                data-attachment-purpose={group.purpose ?? ''}
                data-attachment-actor-role={group.actor?.role ?? ''}
                data-attachment-actor-identity={group.actor?.identity ?? ''}
                data-attachment-actor-name={group.actor?.display_name ?? ''}
              >
                <header>
                  {manageable ? (
                    <button
                      class="ticket-attachments__batch-title"
                      type="button"
                      data-action="edit-attachment-batch-label"
                      aria-label={`Edit batch label ${shownLabel}`}
                      title="Double-click to edit batch label"
                    >
                      {shownLabel}
                    </button>
                  ) : (
                    <h3 class="ticket-attachments__batch-title">{shownLabel}</h3>
                  )}
                  <input
                    class="ticket-attachments__batch-title-editor"
                    value={group.explicitLabel ?? ''}
                    placeholder={group.label}
                    aria-label={`Batch label for ${group.label}`}
                    name="attachment-batch-label"
                    disabled={!manageable}
                  />
                  <select
                    name="attachment-batch-purpose"
                    aria-label={`Purpose for ${group.label}`}
                    disabled={!manageable}
                  >
                    <option value="" selected={!group.purpose}>
                      Uncategorized
                    </option>
                    <option value="problem_evidence" selected={group.purpose === 'problem_evidence'}>
                      Problem evidence
                    </option>
                    <option value="correctness_evidence" selected={group.purpose === 'correctness_evidence'}>
                      Correctness evidence
                    </option>
                    <option value="reference" selected={group.purpose === 'reference'}>
                      Reference
                    </option>
                    <option value="other" selected={group.purpose === 'other'}>
                      Other
                    </option>
                  </select>
                </header>
                {group.items.map((attachment) => (
                  <div
                    class="ticket-inspector__attachment"
                    draggable={manageable ? 'true' : undefined}
                    data-drag-attachment-id={manageable ? attachment.id : undefined}
                    data-component="ticket-attachment-item"
                    data-action={manageable ? 'open-attachment-row' : undefined}
                    data-attachment-id={attachment.id}
                    data-attachment-action-id={manageable ? attachment.id : undefined}
                    data-attachment-name={manageable ? attachment.name : undefined}
                    data-attachment-url={manageable ? attachment.url : undefined}
                    data-attachment-menu-kind={manageable ? 'item' : undefined}
                  >
                    <LucideIcon icon={Paperclip} name="paperclip" />
                    {manageable || !attachment.url ? (
                      <span title={`${attachment.name} — double-click to open`}>{attachment.name}</span>
                    ) : (
                      <a href={attachment.url} target="_blank" rel="noopener" title={`Open ${attachment.name}`}>
                        {attachment.name}
                      </a>
                    )}
                    {manageable && (
                      <button
                        class="ticket-inspector__attachment-menu"
                        type="button"
                        data-action="open-attachment-menu"
                        aria-label={`More actions for ${attachment.name}`}
                        title={`More actions for ${attachment.name}`}
                      >
                        <LucideIcon icon={MoreHorizontal} name="more-horizontal" />
                      </button>
                    )}
                  </div>
                ))}
                {media.length > 0 && (
                  <div
                    class="ticket-attachments__image-grid"
                    aria-label="Attached media"
                    data-attachment-batch-label={group.label}
                  >
                    {media.map((item) => (
                      <button
                        type="button"
                        draggable={manageable ? 'true' : undefined}
                        data-drag-attachment-id={manageable ? item.id : undefined}
                        data-action="open-attachment-gallery"
                        data-gallery-attachment-id={item.id}
                        data-attachment-url={item.url}
                        data-attachment-name={item.name}
                        aria-label={`Open ${item.name} in media gallery${item.annotationCount ? `, ${item.annotationCount} ${item.annotationCount === 1 ? 'annotation' : 'annotations'}` : ''}`}
                      >
                        {isVideoAttachment(item.name) ? (
                          <video
                            draggable="false"
                            src={`${item.url}#t=0.1`}
                            poster={item.thumbnailUrl}
                            data-video-poster-url={item.manageVideoPoster ? item.thumbnailUrl : undefined}
                            data-video-source-url={item.manageVideoPoster ? item.url : undefined}
                            aria-label={item.name}
                            preload="none"
                            muted
                            playsInline
                          />
                        ) : (
                          <img draggable="false" src={item.url} alt={item.name} />
                        )}{' '}
                        {Boolean(item.annotationCount) && (
                          <span
                            class="ticket-attachments__annotation-marker"
                            title={`${item.annotationCount} ${item.annotationCount === 1 ? 'annotation' : 'annotations'}`}
                            aria-hidden="true"
                          >
                            <LucideIcon icon={Pencil} name="pencil" />
                          </span>
                        )}
                      </button>
                    ))}
                  </div>
                )}
              </section>
            );
          })}
        </div>
        {manageable && attachments.length > 0 && (
          <div class="ticket-attachments__new-group" data-attachment-new-group-drop-target="true">
            <LucideIcon icon={Plus} name="plus" />
            <span>New group</span>
          </div>
        )}
        {enabled ? (
          <label class="ticket-attachments__drop">
            <LucideIcon icon={Upload} name="upload" />
            <span>Drop attachments here or browse</span>
            <input type="file" name="ticket-attachments" multiple aria-label="Drop or browse attachments" />
          </label>
        ) : (
          <p class="ticket-attachments__unsupported">This provider does not support attachment actions.</p>
        )}
        {message && (
          <p class="ticket-attachments__status" role="status">
            {message}
          </p>
        )}
      </section>
    </div>
  );
}
