import '@awesome.me/webawesome/dist/components/button/button.js';
import './note-card.css';

import { foregroundColor, uiColor } from '@kerfjs/ui/css-values';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Activity, Check, CircleAlert, FilePenLine, MessageSquareText, RefreshCw, Trash2, X } from 'lucide';

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
  draft?: string;
  readerMode?: boolean;
  respondToFeedback?: boolean;
  inlineReplies?: readonly InlineFeedbackReply[];
  selectedChoices?: readonly string[];
  attachmentContext?: AttachmentReferenceContext;
  aiAuthored?: boolean;
  aiTool?: string;
  aiMayContainErrors?: boolean;
  /** AI completion confidence (0-100) recorded on this note; absent for unscored notes. */
  confidence?: number;
  feedbackFor?: string;
  /** Thumbs ratings associated with this source note, hidden until expanded. */
  aiFeedback?: readonly NoteCardProps[];
  /** `comfortable` (default) uses the canonical card inset; `compact` the tighter list inset. */
  density?: 'comfortable' | 'compact';
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
    <>
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
            </div>
          )}
          {segment.reply && (
            <div class="note-card__inline-reply-row">
              <textarea
                class="note-card__inline-reply"
                name="inline-feedback-response"
                data-note-id={noteId}
                data-offset={sourceStart + segment.reply.offset}
                aria-label={`Response at character ${sourceStart + segment.reply.offset}`}
              >
                {segment.reply.text}
              </textarea>
              <button
                type="button"
                {...INSPECTOR_AND_EDITOR_ACTIONS.removeInlineFeedbackReply.attrs}
                data-note-id={noteId}
                data-offset={sourceStart + segment.reply.offset}
                aria-label={`Remove response at character ${sourceStart + segment.reply.offset}`}
              >
                <LucideIcon size="s" icon={X} name="x" />
              </button>
            </div>
          )}
        </div>
      ))}
    </>
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
  const choiceGroup = parseFeedbackChoices(source),
    selected = new Set(selectedChoices);
  return (
    <div class="note-card__feedback-prompt">
      {choiceGroup ? (
        <>
          <FeedbackBlocks
            source={choiceGroup.before}
            sourceStart={0}
            noteId={id}
            replies={inlineReplies}
            attachmentContext={attachmentContext}
          />
          <div class="note-card__choices" role="group" aria-label="Feedback choices">
            {choiceGroup.choices.map((choice) => (
              <div
                class="note-card__choice"
                {...INSPECTOR_AND_EDITOR_ACTIONS.toggleFeedbackChoice.attrs}
                data-note-id={id}
                data-choice-id={choice.id}
                role="button"
                tabIndex={0}
                aria-pressed={selected.has(choice.id) ? 'true' : 'false'}
              >
                <span class="note-card__choice-check" aria-hidden="true">
                  <LucideIcon
                    size="xs"
                    icon={Check}
                    name="check"
                    color={selected.has(choice.id) ? uiColor('neutral-on-loud') : foregroundColor('transparent')}
                  />
                </span>
                <span>
                  <MarkdownPreview
                    source={choice.markdown}
                    attachmentContext={attachmentContext}
                    size="inherit"
                    media="thumbnail"
                  />
                </span>
              </div>
            ))}
          </div>
          <FeedbackBlocks
            source={choiceGroup.after}
            sourceStart={choiceGroup.afterStart}
            noteId={id}
            replies={inlineReplies}
            attachmentContext={attachmentContext}
          />
        </>
      ) : (
        <FeedbackBlocks
          source={source}
          sourceStart={0}
          noteId={id}
          replies={inlineReplies}
          attachmentContext={attachmentContext}
        />
      )}
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
}: {
  id: string;
  source: string;
  /** Answering a feedback request: response semantics, label, and the "No response needed" action. */
  response?: boolean;
  /** Show the explicit submit actions (reader feedback editors). */
  actions?: boolean;
  placeholder?: string;
}) {
  return (
    <div class="note-card__editor">
      <textarea
        name="note-body"
        data-note-id={id}
        data-note-response={response ? 'true' : undefined}
        aria-label={response ? 'Feedback response' : 'Note body'}
        placeholder={placeholder}
      >
        {source}
      </textarea>
      {actions && (
        <div>
          {response && (
            <wa-button
              size="small"
              appearance="outlined"
              {...INSPECTOR_AND_EDITOR_ACTIONS.dismissFeedback.attrs}
              data-note-id={id}
              title="Clear this feedback request without replying"
            >
              No response needed
            </wa-button>
          )}
          <wa-button
            size="small"
            appearance="accent"
            {...INSPECTOR_AND_EDITOR_ACTIONS.saveNoteEdit.attrs}
            data-note-id={id}
            data-note-response={response ? 'true' : undefined}
          >
            {response ? 'Respond' : 'Submit'}
          </wa-button>
        </div>
      )}
    </div>
  );
}

/** The full-width "Respond to Feedback" action that opens the reader at a feedback request (HS2-3X404M). */
export function RespondToFeedbackButton({ id }: { id: string }) {
  return (
    <wa-button
      class="note-card__respond"
      appearance="outlined"
      {...INSPECTOR_AND_EDITOR_ACTIONS.respondToFeedback.attrs}
      data-note-id={id}
    >
      Respond to Feedback
    </wa-button>
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
  draft,
  readerMode = false,
  respondToFeedback = false,
  inlineReplies = [],
  selectedChoices = [],
  attachmentContext,
  aiAuthored = false,
  aiTool,
  aiMayContainErrors = kind === 'activity',
  confidence,
  aiFeedback = [],
  density = 'comfortable',
}: NoteCardProps) {
  const presentation = presentations[kind];
  const showAiAttribution = aiAuthored && hasAiAuthoredText(kind, body);
  const feedbackEditor = readerMode && (kind === 'feedback_needed' || kind === 'feedback_draft');
  const feedbackResponse = readerMode && kind === 'feedback_needed';
  const editorOpen = editing || feedbackEditor;
  const source = draft ?? (feedbackResponse ? '' : body);
  const choiceGroup = feedbackResponse ? parseFeedbackChoices(body) : undefined;
  const editAttributes =
    editable && !editorOpen
      ? {
          'data-edit-on-click': 'true',
          role: 'button',
          tabIndex: 0,
          'aria-label': 'Edit note',
          title: 'Click to edit',
        }
      : {};
  const acknowledgement = kind === 'regular' && body.trim() === 'No response needed';
  return (
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
          ? `AI-generated ${presentation.label.toLowerCase()} by ${aiTool ?? author}${aiMayContainErrors ? '; may contain errors' : ''}`
          : undefined
      }
      data-acknowledgement={acknowledgement ? 'true' : undefined}
      data-edit-on-click={editable && !editorOpen && !aiFeedback.length ? 'true' : undefined}
      title={editable && !editorOpen ? 'Click to edit' : undefined}
    >
      <header class="note-card__header">
        <span class="note-card__kind">
          <LucideIcon size={15.2} icon={presentation.icon} name={presentation.iconName} />
          {title ?? presentation.label}
        </span>
        <span class="note-card__header-end">
          {!editorOpen && deletable && (
            <span class="note-card__actions">
              <button
                type="button"
                {...INSPECTOR_AND_EDITOR_ACTIONS.deleteNote.attrs}
                data-note-id={id}
                aria-label="Delete note"
              >
                <LucideIcon size={12.8} icon={Trash2} name="trash-2" />
              </button>
            </span>
          )}
          {confidence !== undefined && <ConfidenceBadge value={confidence} />}
          <time class="note-card__time">{time}</time>
        </span>
      </header>
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
          placeholder={
            feedbackResponse && choiceGroup
              ? 'Additional response (optional)'
              : feedbackResponse && inlineReplies.length
                ? 'General response (optional)'
                : undefined
          }
        />
      ) : (
        <div class="note-card__body" {...editAttributes}>
          <MarkdownPreview source={body} attachmentContext={attachmentContext} size="inherit" />
        </div>
      )}
      {respondToFeedback && !readerMode && <RespondToFeedbackButton id={id} />}
      <footer class="note-card__footer">
        {showAiAttribution ? (
          <AIContentLabel tool={aiTool ?? author} mayContainErrors={aiMayContainErrors} feedbackTarget={`note:${id}`} />
        ) : (
          author
        )}
      </footer>
      {aiFeedback.length > 0 && (
        <details class="note-card__ai-feedback">
          <summary>
            <span class="note-card__ai-feedback-show">Show AI Feedback</span>
            <span class="note-card__ai-feedback-hide">Hide AI Feedback</span>
          </summary>
          <div class="note-card__ai-feedback-list">
            {aiFeedback.map((feedback) => (
              <div class="note-card__ai-feedback-entry">
                <NoteCard {...feedback} density="compact" aiFeedback={[]} />
              </div>
            ))}
          </div>
        </details>
      )}
    </article>
  );
}
