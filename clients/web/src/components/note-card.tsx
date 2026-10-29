import './native-textarea.css';
import './note-card.css';

import { Badge } from '@kerfjs/ui/badge';
import { Button } from '@kerfjs/ui/button';
import { ContentItem, type ContentItemAppearance } from '@kerfjs/ui/content-item';
import { rem, uiColor } from '@kerfjs/ui/css-values';
import { DisclosureArrow } from '@kerfjs/ui/disclosure-arrow';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';
import { Activity, CircleAlert, FilePenLine, MessageSquareText, Pencil, Plus, RefreshCw, Trash2, X } from 'lucide';

import type { AttachmentReferenceContext } from '../attachment-references';
import { parseFeedbackChoices } from '../feedback-choices';
import { type InlineFeedbackReply, splitFeedbackPrompt } from '../feedback-replies';
import { INSPECTOR_AND_EDITOR_ACTIONS } from '../interaction-attrs/inspector-and-editor';
import { AIContentLabel } from './ai-content-label';
import { ConfidenceBadge } from './confidence-badge';
import { MarkdownPreview } from './markdown-preview';

export { ConfidenceBadge, type ConfidenceBand, confidenceBand } from './confidence-badge';

export type NoteKind = 'regular' | 'status' | 'feedback_needed' | 'feedback_draft' | 'activity';
export interface NoteCardProps {
  id: string;
  kind: NoteKind;
  author: string;
  time: string;
  body: string;
  title?: string;
  editable?: boolean;
  deletable?: boolean;
  editing?: boolean;
  /** The edit open on this FEEDBACK NEEDED prompt is an explicit Edit Note, not a reply draft (HS2-ZXGEFE). */
  editingPrompt?: boolean;
  /** Captured local response purpose survives a remote kind correction (HS2-VBFSZJ). */
  responseDraft?: boolean;
  responseRemoved?: boolean;
  draft?: string;
  readerMode?: boolean;
  /** The stored note is an AI FEEDBACK NEEDED prompt, even when an answer presents it as regular. */
  feedbackPrompt?: boolean;
  respondToFeedback?: boolean;
  inlineReplies?: readonly InlineFeedbackReply[];
  selectedChoices?: readonly string[];
  attachmentContext?: AttachmentReferenceContext;
  aiAuthored?: boolean;
  humanEdited?: boolean;
  aiTool?: string;
  aiMayContainErrors?: boolean;
  /** AI completion confidence (0-100) recorded on this note; absent for unscored notes. */
  confidence?: number;
  feedbackFor?: string;
  ratingMetadata?: { target: string; rating: 'helpful' | 'not_helpful' | null; explanation?: string };
  /** Thumbs ratings associated with this source note, hidden until expanded. */
  aiFeedback?: readonly NoteCardProps[];
  /** `comfortable` (default) uses the canonical card inset; `compact` the tighter list inset. */
  density?: 'comfortable' | 'compact';
  /**
   * Whether the note paints its own Kerf ContentItem frame (default). A note nested inside another
   * note's AI-feedback disclosure renders unframed so the parent frame is the only inset.
   */
  framed?: boolean;
}

/** HS2-PATEA0 kind mapping onto Kerf ContentItem appearances. */
function noteAppearance(kind: NoteKind, acknowledgement: boolean): ContentItemAppearance {
  if (acknowledgement || kind === 'status') return 'neutral';
  if (kind === 'activity') return 'transparent';
  if (kind === 'feedback_needed' || kind === 'feedback_draft') return 'warning';
  return 'surface';
}

const presentations = {
  regular: { label: 'Note', icon: MessageSquareText, iconName: 'message-square-text' },
  status: { label: 'Status update', icon: RefreshCw, iconName: 'refresh-cw' },
  feedback_needed: { label: 'Feedback needed', icon: CircleAlert, iconName: 'circle-alert' },
  feedback_draft: { label: 'Feedback draft', icon: FilePenLine, iconName: 'file-pen-line' },
  activity: { label: 'Activity', icon: Activity, iconName: 'activity' },
} as const;

/** Keep automatic event labels distinct from authored prose, including in mixed activity notes. */
function hasAiAuthoredText(kind: NoteKind, body: string): boolean {
  const text = body.trim();
  if (!text) return false;
  if (kind === 'status') return /\r?\n\s*\S/u.test(text);
  if (kind === 'activity' && /^Status changed from [^\r\n]+ to [^\r\n]+$/u.test(text)) return false;
  return true;
}

/** Provenance is stored in the note for idempotency, but is not part of its prose. */
function visibleNoteBody(kind: NoteKind, body: string): string {
  if (kind !== 'activity') return body;
  return body.replace(/\r?\n\r?\n<!-- hotsheet:activity-distillation:v1:[A-Za-z0-9_-]+ -->\s*$/u, '');
}

function FeedbackBlocks({
  source,
  sourceStart,
  noteId,
  replies,
  attachmentContext,
}: {
  source: string;
  sourceStart: number;
  noteId: string;
  replies: readonly InlineFeedbackReply[];
  attachmentContext?: AttachmentReferenceContext;
}) {
  if (!source) return null;
  const localReplies = replies
    .filter((reply) => reply.offset >= sourceStart && reply.offset <= sourceStart + source.length)
    .map((reply) => ({ ...reply, offset: reply.offset - sourceStart }));
  return (
    <List gap="2xs">
      {splitFeedbackPrompt(source, localReplies).map((segment) => (
        <div class="note-card__feedback-section">
          {segment.markdown && (
            <div
              class="note-card__feedback-block"
              {...INSPECTOR_AND_EDITOR_ACTIONS.addInlineFeedbackReply.attrs}
              data-note-id={noteId}
              data-segment-start={sourceStart + segment.start}
              data-segment-end={sourceStart + segment.end}
              role="button"
              tabIndex={0}
              aria-label="Add response at a character position"
            >
              <MarkdownPreview source={segment.markdown} attachmentContext={attachmentContext} />
              <span class="note-card__feedback-add" aria-hidden="true">
                <Badge tone="info" size="compact" ariaHidden icon={<LucideIcon size="xs" icon={Plus} name="plus" />}>
                  {''}
                </Badge>
              </span>
            </div>
          )}
          {segment.reply && (
            <div class="note-card__inline-reply-row">
              <textarea
                class="native-textarea note-card__inline-reply"
                data-tone="brand"
                name="inline-feedback-response"
                data-note-id={noteId}
                data-offset={sourceStart + segment.reply.offset}
                aria-label={`Response at character ${sourceStart + segment.reply.offset}`}
              >
                {segment.reply.text}
              </textarea>
              <Button
                size="compact"
                emphasis="plain"
                action={INSPECTOR_AND_EDITOR_ACTIONS.removeInlineFeedbackReply.value}
                rootAttributes={{
                  'data-note-id': noteId,
                  'data-offset': String(sourceStart + segment.reply.offset),
                }}
                ariaLabel={`Remove Response at Character ${sourceStart + segment.reply.offset}`}
                title="Remove response"
                icon={<LucideIcon size="s" icon={X} name="x" />}
              />
            </div>
          )}
        </div>
      ))}
    </List>
  );
}

export function FeedbackPrompt({
  source,
  id,
  inlineReplies = [],
  selectedChoices = [],
  attachmentContext,
}: {
  source: string;
  id: string;
  inlineReplies?: readonly InlineFeedbackReply[];
  selectedChoices?: readonly string[];
  attachmentContext?: AttachmentReferenceContext;
}) {
  const choices = parseFeedbackChoices(source),
    selected = new Set(selectedChoices);
  if (!choices)
    return (
      <div class="note-card__feedback-prompt">
        <List gap="2xs">
          <FeedbackBlocks
            source={source}
            sourceStart={0}
            noteId={id}
            replies={inlineReplies}
            attachmentContext={attachmentContext}
          />
        </List>
      </div>
    );
  const trailing = choices.regions[choices.groups.length];
  return (
    <div class="note-card__feedback-prompt">
      <List gap="2xs">
        {choices.groups.map((group, index) => (
          <>
            <FeedbackBlocks
              source={choices.regions[index].source}
              sourceStart={choices.regions[index].sourceStart}
              noteId={id}
              replies={inlineReplies}
              attachmentContext={attachmentContext}
            />
            {group.inlineQuestion && group.question && (
              // A `CHOICE: question` header carries its question on the header line (HS2-3MHSBS).
              <div class="note-card__feedback-question">
                <MarkdownPreview source={group.question} attachmentContext={attachmentContext} />
              </div>
            )}
            <List
              selectionMode="multiple"
              ariaLabel={
                choices.groups.length === 1
                  ? 'Feedback Choices'
                  : `Feedback Choices ${group.index}${group.question ? `: ${group.question}` : ''}`
              }
              gap="xs"
            >
              {group.choices.map((choice) => (
                <ContentItem
                  interactive
                  action={INSPECTOR_AND_EDITOR_ACTIONS.toggleFeedbackChoice.value}
                  itemId={choice.id}
                  selectionMode="multiple"
                  selected={selected.has(choice.id)}
                  frame="framed"
                  rootAttributes={{ 'data-note-id': id, 'data-choice-id': choice.id }}
                >
                  <MarkdownPreview
                    source={choice.markdown}
                    attachmentContext={attachmentContext}
                    size="inherit"
                    media="thumbnail"
                  />
                </ContentItem>
              ))}
            </List>
          </>
        ))}
        <FeedbackBlocks
          source={trailing.source}
          sourceStart={trailing.sourceStart}
          noteId={id}
          replies={inlineReplies}
          attachmentContext={attachmentContext}
        />
      </List>
    </div>
  );
}

/**
 * A note's body editor (HS2-3X404M): the textarea plus, for a reader feedback editor, its "No response
 * needed" and Respond/Submit actions. `NoteCard` renders it while editing, and hosts that answer a
 * feedback request outside a note card (the marked ticket description in `TicketInfoPanel`) compose it
 * instead of borrowing NoteCard's classes. `id` is the note (or details-feedback) id the actions target.
 */
export function NoteEditor({
  id,
  source,
  response = false,
  actions = false,
  placeholder,
  dismissible = true,
}: {
  id: string;
  source: string;
  /** Answering a feedback request: response semantics, label, and the "No response needed" action. */
  response?: boolean;
  /** Show the explicit submit actions (reader feedback editors). */
  actions?: boolean;
  placeholder?: string;
  dismissible?: boolean;
}) {
  return (
    <div class="note-card__editor">
      <textarea
        class="native-textarea"
        name="note-body"
        data-note-id={id}
        data-note-response={response ? 'true' : undefined}
        aria-label={response ? 'Feedback response' : 'Note body'}
        placeholder={placeholder}
      >
        {source}
      </textarea>
      {actions && (
        <Row hAlign="right" vAlign="middle" gap="xs">
          {response && dismissible && (
            <Button
              size="compact"
              emphasis="secondary"
              action={INSPECTOR_AND_EDITOR_ACTIONS.dismissFeedback.value}
              title="Clear this feedback request without replying"
              rootAttributes={{ 'data-note-id': id }}
            >
              No Response Needed
            </Button>
          )}
          <Button
            size="compact"
            emphasis="primary"
            action={INSPECTOR_AND_EDITOR_ACTIONS.saveNoteEdit.value}
            rootAttributes={{ 'data-note-id': id, 'data-note-response': response ? 'true' : undefined }}
          >
            {response ? 'Respond' : 'Submit'}
          </Button>
        </Row>
      )}
    </div>
  );
}

/** The full-width "Respond to Feedback" action that opens the reader at a feedback request (HS2-3X404M). */
export function RespondToFeedbackButton({ id }: { id: string }) {
  return (
    <div class="note-card__respond">
      <Button
        emphasis="secondary"
        action={INSPECTOR_AND_EDITOR_ACTIONS.respondToFeedback.value}
        rootAttributes={{ 'data-note-id': id }}
      >
        Respond to Feedback
      </Button>
    </div>
  );
}

export function NoteCard({
  id,
  kind,
  author,
  time,
  body,
  title,
  editable = true,
  deletable = true,
  editing = false,
  editingPrompt = false,
  responseDraft = false,
  responseRemoved = false,
  draft,
  readerMode = false,
  feedbackPrompt = kind === 'feedback_needed',
  respondToFeedback = false,
  inlineReplies = [],
  selectedChoices = [],
  attachmentContext,
  aiAuthored = false,
  humanEdited = false,
  aiTool,
  aiMayContainErrors = kind === 'activity',
  confidence,
  aiFeedback = [],
  density = 'comfortable',
  framed = true,
}: NoteCardProps) {
  const presentation = presentations[kind];
  const showAiAttribution = !responseRemoved && aiAuthored && hasAiAuthoredText(kind, body);
  // Editing the prompt itself replaces the response editor with the ordinary note editor (HS2-ZXGEFE).
  const promptEdit = editing && editingPrompt && feedbackPrompt;
  const feedbackEditor =
    readerMode && !promptEdit && (responseDraft || kind === 'feedback_needed' || kind === 'feedback_draft');
  const feedbackResponse = readerMode && !promptEdit && (responseDraft || kind === 'feedback_needed');
  const editorOpen = editing || feedbackEditor;
  // An AI FEEDBACK NEEDED prompt is answered with a new note, never rewritten by a plain click
  // (HS2-MB2Q10); only its explicit Edit Note action or a double-click opens it (HS2-ZXGEFE).
  const editInPlace = editable && !feedbackPrompt && !responseRemoved;
  const editPrompt = editable && feedbackPrompt && !editing && !responseRemoved;
  const source = draft ?? (feedbackResponse ? '' : body);
  const choiceGroup = feedbackResponse ? parseFeedbackChoices(body) : undefined;
  const editAttributes =
    editInPlace && !editorOpen
      ? {
          'data-edit-on-click': 'true',
          role: 'button',
          tabIndex: 0,
          'aria-label': 'Edit note',
          title: 'Click to edit',
        }
      : {};
  const acknowledgement = kind === 'regular' && body.trim() === 'No response needed';
  const card = (
    <article
      class={`note-card${editorOpen ? ' note-card--editing' : ''}`}
      data-component="note-card"
      data-note-id={id}
      data-kind={kind}
      data-density={density === 'compact' ? 'compact' : undefined}
      data-confidence={confidence !== undefined ? String(confidence) : undefined}
      data-ai-authored={aiAuthored ? 'true' : undefined}
      aria-label={
        showAiAttribution
          ? `AI-generated ${presentation.label.toLowerCase()} by ${aiTool ?? author}${humanEdited ? '; edited by human' : ''}${aiMayContainErrors ? '; may contain errors' : ''}`
          : undefined
      }
      data-acknowledgement={acknowledgement ? 'true' : undefined}
      data-edit-on-click={editInPlace && !editorOpen && !aiFeedback.length ? 'true' : undefined}
      title={editInPlace && !editorOpen ? 'Click to edit' : undefined}
    >
      <Row className="note-card__header" hAlign="space-between" vAlign="middle" gap="m">
        <strong class="note-card__kind">
          <Text
            variant="span"
            size="compact"
            color={
              kind === 'feedback_needed' || kind === 'feedback_draft'
                ? uiColor('warning-on-quiet')
                : kind === 'status'
                  ? uiColor('text-link')
                  : kind === 'activity' || acknowledgement
                    ? uiColor('neutral-on-quiet')
                    : undefined
            }
          >
            <LucideIcon inline size={15.2} icon={presentation.icon} name={presentation.iconName} />{' '}
            {title ?? presentation.label}
          </Text>
        </strong>
        <Row className="note-card__header-end" vAlign="middle" gap="2xs">
          {(editPrompt || (!editorOpen && deletable)) && (
            <span class="note-card__actions">
              {editPrompt && (
                <Button
                  size="compact"
                  emphasis="plain"
                  action={INSPECTOR_AND_EDITOR_ACTIONS.editFeedbackPrompt.value}
                  rootAttributes={{ 'data-note-id': id }}
                  ariaLabel="Edit Note"
                  title="Edit note"
                  icon={<LucideIcon size={12.8} icon={Pencil} name="pencil" />}
                />
              )}
              {!editorOpen && deletable && (
                <Button
                  size="compact"
                  emphasis="plain"
                  action={INSPECTOR_AND_EDITOR_ACTIONS.deleteNote.value}
                  rootAttributes={{ 'data-note-id': id }}
                  ariaLabel="Delete Note"
                  title="Delete note"
                  icon={<LucideIcon size={12.8} icon={Trash2} name="trash-2" />}
                />
              )}
            </span>
          )}
          {confidence !== undefined && <ConfidenceBadge value={confidence} />}
          <time class="note-card__time">
            <Text variant="span" size="compact" tone="quiet">
              {time}
            </Text>
          </time>
        </Row>
      </Row>
      {feedbackResponse && (
        <FeedbackPrompt
          source={body}
          id={id}
          inlineReplies={inlineReplies}
          selectedChoices={selectedChoices}
          attachmentContext={attachmentContext}
        />
      )}
      {editorOpen ? (
        <NoteEditor
          id={id}
          source={source}
          response={feedbackResponse}
          actions={feedbackEditor}
          dismissible={!responseRemoved}
          placeholder={
            feedbackResponse && choiceGroup
              ? 'Additional response (optional)'
              : feedbackResponse && inlineReplies.length
                ? 'General response (optional)'
                : undefined
          }
        />
      ) : (
        <div
          class="note-card__body"
          {...editAttributes}
          data-feedback-prompt-body={editPrompt ? 'true' : undefined}
          title={editPrompt ? 'Double-click to edit' : editAttributes.title}
        >
          <MarkdownPreview source={visibleNoteBody(kind, body)} attachmentContext={attachmentContext} size="inherit" />
        </div>
      )}
      {respondToFeedback && !readerMode && !promptEdit && <RespondToFeedbackButton id={id} />}
      <footer class="note-card__footer">
        {showAiAttribution ? (
          <AIContentLabel
            tool={aiTool ?? author}
            humanEdited={humanEdited}
            mayContainErrors={aiMayContainErrors}
            feedbackTarget={`note:${id}`}
          />
        ) : (
          <Text variant="span" size="compact" tone="quiet">
            {author}
          </Text>
        )}
      </footer>
      {aiFeedback.length > 0 && (
        <details class="note-card__ai-feedback">
          <summary>
            {/* The native details owns open state, so each label carries its own controlled arrow. */}
            <span class="note-card__ai-feedback-show">
              <DisclosureArrow open={false} size={rem(0.875)} />
              Show AI Feedback
            </span>
            <span class="note-card__ai-feedback-hide">
              <DisclosureArrow open size={rem(0.875)} />
              Hide AI Feedback
            </span>
          </summary>
          <div class="note-card__ai-feedback-list">
            {aiFeedback.map((feedback) => (
              <div class="note-card__ai-feedback-entry">
                <NoteCard {...feedback} density="compact" aiFeedback={[]} framed={false} />
              </div>
            ))}
          </div>
        </details>
      )}
    </article>
  );
  // The frame carries the note's morph key so a re-render (opening the editor) keeps the same nodes.
  return framed ? (
    <ContentItem appearance={noteAppearance(kind, acknowledgement)} rootAttributes={{ 'data-key': `note-${id}` }}>
      {card}
    </ContentItem>
  ) : (
    card
  );
}
