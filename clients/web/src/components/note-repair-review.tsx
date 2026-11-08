import './note-repair-review.css';

import { Button } from '@kerfjs/ui/button';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { StateBanner } from '@kerfjs/ui/state-banner';

import type { NoteDoctorReport } from '../api';
import { INSPECTOR_AND_EDITOR_ACTIONS } from '../interaction-attrs/inspector-and-editor';
import type { NoteRepairPreview } from '../note-repair-proposal';

export interface NoteRepairReviewProps {
  noteId: string;
  report: NoteDoctorReport;
  before: string;
  beforeKind: string;
  response?: string;
  preview?: NoteRepairPreview;
  issue?: string;
  stale?: boolean;
  applying?: boolean;
}

/** The AI transcript stays visible beside this explicit human review (HS2-5E8Y9R). */
export function NoteRepairReview({
  noteId,
  report,
  before,
  beforeKind,
  response,
  preview,
  issue,
  stale,
  applying,
}: NoteRepairReviewProps) {
  const proposal = preview?.proposal;
  return (
    <section class="note-repair-review" data-component="note-repair-review" data-note-id={noteId}>
      <List gap="xs">
        <strong>Review note repair</strong>
        {report.findings.length ? (
          <div>
            <strong>Doctor findings</strong>
            <ul>
              {report.findings.map((finding) => (
                <li>{finding.description}</li>
              ))}
            </ul>
          </div>
        ) : (
          <p>The deterministic doctor found no known structural issue.</p>
        )}
        {!response && <p>Waiting for the AI proposal. The note will not change until you accept it.</p>}
        {issue ? <StateBanner tone="warning" title="Proposal Needs Review" detail={issue} /> : null}
        {stale && (
          <StateBanner
            tone="warning"
            title="Ticket Changed"
            detail="Close this review and start a fresh repair from the current note."
          />
        )}
        {proposal && (
          <div class="note-repair-review__comparison">
            {proposal.text !== undefined && proposal.text !== before ? (
              <>
                <div>
                  <strong>Before</strong>
                  <pre>{before}</pre>
                </div>
                <div>
                  <strong>After</strong>
                  <pre>{proposal.text}</pre>
                </div>
              </>
            ) : (
              <p>Text unchanged.</p>
            )}
            {proposal.kind !== undefined && (
              <p>
                Type: {beforeKind} → {proposal.kind}
              </p>
            )}
            {proposal.confidence !== undefined && <p>Confidence: {proposal.confidence ?? 'clear'}</p>}
            {proposal.summary !== undefined && <p>Summary: {proposal.summary ?? 'clear'}</p>}
            {proposal.response_to !== undefined && <p>Response link: {proposal.response_to ?? 'clear'}</p>}
            {proposal.explanation && <p>{proposal.explanation}</p>}
            {preview.unavailable.map((reason) => (
              <StateBanner tone="warning" title="Cannot Apply" detail={reason} />
            ))}
          </div>
        )}
        <Row gap="xs" wrap>
          <Button emphasis="plain" action={INSPECTOR_AND_EDITOR_ACTIONS.dismissNoteRepair.value}>
            Reject and Close
          </Button>
          {preview && (
            <Button
              emphasis="primary"
              action={INSPECTOR_AND_EDITOR_ACTIONS.applyNoteRepair.value}
              disabled={applying || stale || !preview.changed || preview.unavailable.length > 0}
            >
              Apply Repair
            </Button>
          )}
        </Row>
      </List>
    </section>
  );
}
