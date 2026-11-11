import './stats-projects-table.css';

import { Button } from '@kerfjs/ui/button';
import { type CssForegroundColor, foregroundColorVar } from '@kerfjs/ui/css-values';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Text } from '@kerfjs/ui/text';

import type { LiveWord } from '../api';
import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import { TERMINALS_ACTIONS } from '../interaction-attrs/terminals';
import { LIVE_STATE_ICONS } from '../live-state-icons';
import { formatDuration, LIVE_WORD_LABELS, type RightNowRow, type RightNowWork } from '../stats-right-now';
import { formatGlanceValue } from './glance-gauge-geometry';

export interface StatsProjectsTableProps {
  rows: readonly RightNowRow[];
}

/** Dark cockpit (§34.1): a word is coloured only by what it means; Stopped stays quiet. */
const WORD_COLORS: Record<LiveWord, CssForegroundColor | undefined> = {
  active: foregroundColorVar('--kui-color-success-on-quiet'),
  waiting_on_you: foregroundColorVar('--kui-color-brand-on-quiet'),
  may_be_stuck: foregroundColorVar('--kui-color-warning-on-quiet'),
  stalled: foregroundColorVar('--kui-color-danger-on-quiet'),
  halted: foregroundColorVar('--kui-color-danger-on-quiet'),
  stopped: undefined,
};

const OVERRUN_COLOR = foregroundColorVar('--kui-color-warning-on-quiet');

/** `final_testing` reads "final testing (CI)": the ticket is waiting on its remote CI run. */
function phaseLabel(phase: string | undefined): string | undefined {
  if (!phase) return undefined;
  const words = phase.replace(/_/gu, ' ');
  return phase === 'final_testing' ? `${words} (CI)` : words;
}

function workLine(work: RightNowWork): string {
  const phase = phaseLabel(work.phase);
  return phase ? `${work.slug} · ${phase}` : work.slug;
}

function idleWork(row: RightNowRow): string {
  if (row.waiting > 0) return `${row.waiting} ${row.waiting === 1 ? 'request' : 'requests'} waiting on you`;
  return 'Nothing claimed';
}

/**
 * The Stats Right now **Projects** table (HS2-ERTNJ7, mock `m3-stats-right-now`): expectation versus
 * reality per project, sorted by need, with the one action that unsticks it. A native table: Kerf has
 * no tabular-data component, and the numeric columns need shared tracks. It scrolls sideways inside
 * its own container when the page is narrow.
 */
export function StatsProjectsTable({ rows }: StatsProjectsTableProps) {
  return (
    <div class="stats-projects-table" data-component="stats-projects-table">
      <table class="stats-projects-table__table">
        <caption class="stats-projects-table__caption">Projects, sorted by need</caption>
        <thead>
          <tr>
            <th scope="col">
              <Text variant="span" size="compact" tone="quiet" weight="strong" flush wrap="nowrap">
                State
              </Text>
            </th>
            <th scope="col">
              <Text variant="span" size="compact" tone="quiet" weight="strong" flush wrap="nowrap">
                Project
              </Text>
            </th>
            <th scope="col">
              <Text variant="span" size="compact" tone="quiet" weight="strong" flush wrap="nowrap">
                Current work
              </Text>
            </th>
            <th scope="col">
              <Text variant="span" size="compact" tone="quiet" weight="strong" flush wrap="nowrap">
                Last progress
              </Text>
            </th>
            <th scope="col">
              <Text variant="span" size="compact" tone="quiet" weight="strong" flush wrap="nowrap">
                ETA
              </Text>
            </th>
            <th scope="col">
              <Text variant="span" size="compact" tone="quiet" weight="strong" flush wrap="nowrap">
                Tokens/min
              </Text>
            </th>
            <th scope="col">
              <span class="stats-projects-table__caption">Action</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => {
            const icon = LIVE_STATE_ICONS[row.state];
            const label = LIVE_WORD_LABELS[row.state];
            return (
              <tr data-key={`right-now:${row.store}`} data-store={row.store} data-state={row.state}>
                <td>
                  <span class="stats-projects-table__state">
                    <LucideIcon icon={icon.icon} name={icon.name} color={WORD_COLORS[row.state]} size="s" />
                    <Text variant="span" color={WORD_COLORS[row.state]} tone="quiet" flush wrap="nowrap">
                      {label}
                    </Text>
                  </span>
                </td>
                <th scope="row">
                  <Text variant="span" weight="strong" flush wrap="nowrap">
                    {row.name}
                  </Text>
                </th>
                <td>
                  <span class="stats-projects-table__work">
                    {row.work.length ? (
                      row.work.map((work, index) => (
                        <Text
                          variant="span"
                          size={index === 0 ? 'default' : 'compact'}
                          tone={index === 0 ? 'default' : 'quiet'}
                          flush
                          title={work.title}
                          rootAttributes={{ 'data-ticket': work.slug }}
                        >
                          {workLine(work)}
                        </Text>
                      ))
                    ) : (
                      <Text variant="span" tone="quiet" flush>
                        {idleWork(row)}
                      </Text>
                    )}
                  </span>
                </td>
                <td data-column="progress">
                  {row.progressAgeMs === undefined ? '—' : `${formatDuration(row.progressAgeMs)} ago`}
                </td>
                <td data-column="eta">
                  {row.etaOverrunMs === undefined ? (
                    '—'
                  ) : (
                    <Text variant="span" color={OVERRUN_COLOR} flush title="Past the claim's ETA">
                      +{formatDuration(row.etaOverrunMs)}
                    </Text>
                  )}
                </td>
                <td data-column="tokens" title={`${Math.round(row.tokensPerMinute)} tokens per minute`}>
                  {formatGlanceValue(row.tokensPerMinute)}
                </td>
                <td data-column="action">
                  {row.action?.kind === 'answer' && (
                    <Button
                      emphasis="link"
                      size="compact"
                      action={NOTIFICATIONS_AND_LINKS_ACTIONS.openLinkedTicket.value}
                      title={`Answer ${row.action.slug}`}
                      rootAttributes={{
                        'data-ticket-slug': row.action.slug,
                        'data-ticket-project-id': row.action.projectId,
                      }}
                    >
                      Answer
                    </Button>
                  )}
                  {row.action?.kind === 'open-terminal' && (
                    <Button
                      emphasis="link"
                      size="compact"
                      action={TERMINALS_ACTIONS.openTerminalProject.value}
                      title={`Open the ${row.name} terminal`}
                      rootAttributes={{ 'data-terminal-key': row.action.terminalKey }}
                    >
                      Open Terminal
                    </Button>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
