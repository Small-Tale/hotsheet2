import './repository-size.css';

import { Badge } from '@kerfjs/ui/badge';
import { Button } from '@kerfjs/ui/button';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';
import { CircleAlert, HardDrive, OctagonAlert, TriangleAlert } from 'lucide';

import type {
  CompactionPlan,
  CompactionResult,
  RepositorySizeEntry,
  RepositorySizeLevel,
  RepositorySizeReport,
} from '../api';
import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';

/** What the repository size section shows (HS2-QF44T7). */
export type RepositorySizeState =
  { status: 'loading' } | { status: 'error'; message: string } | { status: 'ready'; report: RepositorySizeReport };

export type CompactionPreviewState =
  | { status: 'idle' }
  | { status: 'loading'; source: string }
  | { status: 'error'; source: string; message: string }
  | {
      status: 'ready';
      source: string;
      plan: CompactionPlan;
      execution?:
        { status: 'running' } | { status: 'error'; message: string } | { status: 'success'; result: CompactionResult };
    };

const SIZE_UNITS = ['B', 'kB', 'MB', 'GB', 'TB'] as const;

/**
 * A byte count in decimal units, matching how GitHub states its limits (`5 GB`) and the
 * CLI's `checkout repo-size` text: one decimal below 100, none above, and no trailing `.0`.
 */
export function formatBytes(bytes: number): string {
  if (bytes < 1000) return `${Math.max(0, Math.round(bytes))} B`;
  let value = bytes,
    unit = 0;
  while (value >= 1000 && unit < SIZE_UNITS.length - 1) {
    value /= 1000;
    unit += 1;
  }
  const number = value >= 100 ? value.toFixed(0) : value.toFixed(1);
  return `${number.replace(/\.0$/u, '')} ${SIZE_UNITS[unit]}`;
}

/** A count with thousands separators, independent of the browser locale. */
function formatCount(count: number): string {
  return Math.round(count).toLocaleString('en-US');
}

function plural(count: number, one: string, many: string): string {
  return `${formatCount(count)} ${count === 1 ? one : many}`;
}

/** The entry's display name, including what kind of repository it is. */
export function repositorySizeEntryTitle(entry: RepositorySizeEntry): string {
  return entry.kind === 'code' ? `Code repository: ${entry.label}` : `Ticket store: ${entry.label}`;
}

const LEVEL_BADGES: Record<RepositorySizeLevel, { tone: 'success' | 'warning' | 'danger'; text: string }> = {
  ok: { tone: 'success', text: 'Within limits' },
  warning: { tone: 'warning', text: 'Over soft limit' },
  critical: { tone: 'danger', text: 'Over hard limit' },
};

const STATUS_BADGES: Record<Exclude<RepositorySizeEntry['status'], 'measured'>, string> = {
  not_repository: 'Not a Git repository',
  unsupported: 'Not measured',
  unavailable: 'Unavailable',
};

function entryBadge(entry: RepositorySizeEntry) {
  if (entry.status !== 'measured')
    return (
      <Badge tone="neutral" size="compact" rootAttributes={{ 'data-size-status': entry.status }}>
        {STATUS_BADGES[entry.status]}
      </Badge>
    );
  const badge = LEVEL_BADGES[entry.level];
  return (
    <Badge tone={badge.tone} size="compact" rootAttributes={{ 'data-size-level': entry.level }}>
      {badge.text}
    </Badge>
  );
}

function gitValue(entry: RepositorySizeEntry): string {
  const git = entry.git;
  if (!git) return '—';
  const objects = git.loose_objects + git.packed_objects;
  return `${formatBytes(git.total_bytes)} (${plural(objects, 'object', 'objects')})`;
}

function attachmentsValue(entry: RepositorySizeEntry): string {
  const attachments = entry.attachments;
  if (!attachments) return '—';
  const size = `${attachments.truncated ? 'At least ' : ''}${formatBytes(attachments.bytes)}`;
  return `${size} in ${plural(attachments.files, 'file', 'files')}`;
}

function EntryRows({ entry }: { entry: RepositorySizeEntry }) {
  if (entry.status === 'unsupported')
    return <ValueTableRow label="Size" value={entry.detail ?? 'This source has no local repository to measure.'} />;
  if (entry.status !== 'measured')
    return (
      <>
        <ValueTableRow label="Size" value={entry.detail ?? 'The repository could not be measured.'} />
        {entry.attachments && <ValueTableRow label="Attachments" value={attachmentsValue(entry)} />}
      </>
    );
  return (
    <>
      <ValueTableRow
        label="Git objects"
        value={entry.shared_with_code ? `Shared with the code repository: ${gitValue(entry)}` : gitValue(entry)}
      />
      {entry.kind === 'ticket_store' && <ValueTableRow label="Attachments" value={attachmentsValue(entry)} />}
    </>
  );
}

function RepositoryEntry({ entry }: { entry: RepositorySizeEntry }) {
  const title = repositorySizeEntryTitle(entry);
  return (
    <section
      class="repository-size__entry"
      aria-label={title}
      data-key={`repository-size:${entry.kind}:${entry.connection_id ?? 'code'}`}
      data-kind={entry.kind}
      data-status={entry.status}
      data-level={entry.level}
    >
      <List gap="2xs">
        {/* Size m: a heading-weight label that keeps a store or alias name's exact case. */}
        <ListHeader label={title} headingLevel={3} size="m" />
        <ValueTable label={`${title} size`} stackBelow="compact">
          <ValueTableRow label="Status" value={entryBadge(entry)} />
          <EntryRows entry={entry} />
          <ValueTableRow
            label="Location"
            value={
              <Text
                variant="span"
                font="monospace"
                size="compact"
                border="none"
                wrap="truncate"
                truncate="middle"
                title={entry.locator}
              >
                {entry.locator}
              </Text>
            }
          />
        </ValueTable>
        {entry.kind === 'ticket_store' &&
          entry.provider === 'git' &&
          entry.status === 'measured' &&
          !entry.shared_with_code &&
          entry.connection_id !== undefined && (
            <Button
              tone="brand"
              emphasis="secondary"
              size="compact"
              action={PROJECT_LIFECYCLE_ACTIONS.previewRepositoryCompaction.value}
              rootAttributes={{ 'data-source': entry.connection_id }}
            >
              Preview Attachment Compaction
            </Button>
          )}
      </List>
    </section>
  );
}

function CompactionPreview({ state, checkout }: { state: CompactionPreviewState; checkout: string }) {
  if (state.status === 'idle') return null;
  if (state.status === 'loading') return <EmptyState busy title={`Checking ${state.source} Attachments`} />;
  if (state.status === 'error')
    return <StateBanner tone="warning" title="Compaction preview unavailable." detail={state.message} />;
  const { plan, source } = state;
  const command = `hotsheet-cli checkout compact '${source.replaceAll("'", "'\\''")}' '${checkout.replaceAll("'", "'\\''")}'`;
  const required = `rewrite ${plan.branch} ${plan.fingerprint}`;
  return (
    <section class="repository-size__compaction" aria-label={`${source} Compaction Preview`} data-state="ready">
      <List gap="xs">
        <Text variant="h3" size="large" flush>
          Attachment compaction: {source}
        </Text>
        <Text variant="p" tone="quiet" flush>
          {plan.candidates.length} candidates, {formatBytes(plan.reclaimable_payload_bytes)} of attachment payload
          history. Attached candidates are on tickets at least {plan.min_age_days} days old; oversized means at least{' '}
          {formatBytes(plan.oversized_bytes)}.
        </Text>
        {plan.candidates.map((item) => (
          <Text variant="p" flush data-key={`compaction:${item.path}`}>
            {item.reason === 'orphaned' ? 'Removed attachment' : item.ticket_slug}: {item.filename} (
            {formatBytes(item.bytes)}, {item.reason})
          </Text>
        ))}
        <Text variant="p" tone="quiet" flush>
          This rewrites the ticket store's Git history and force-pushes the selected branch. Save a complete mirror
          backup outside the ticket store. Every collaborator must preserve local work and re-clone or reset afterward.
        </Text>
        {plan.candidates.length === 0 ? (
          <StateBanner tone="info" title="No attachments are eligible for compaction." />
        ) : state.execution?.status === 'success' ? (
          <List gap="xs">
            <StateBanner
              tone="success"
              title="Compaction pushed to origin."
              detail={`${state.execution.result.candidates_removed} attachments removed; ${formatBytes(state.execution.result.git_bytes_before)} to ${formatBytes(state.execution.result.git_bytes_after)} of Git data.`}
            />
            <Text variant="p" flush>
              Backup: <code>{state.execution.result.backup}</code>
            </Text>
            <Text variant="p" flush>
              New head: <code>{state.execution.result.new_head}</code>
            </Text>
            {state.execution.result.recovery.map((step) => (
              <Text variant="p" flush>
                {step}
              </Text>
            ))}
            <Text variant="p" flush>
              After restoring this checkout to origin, run <code>{command} --recover</code> to clear its write guard.
            </Text>
          </List>
        ) : (
          <form {...PROJECT_LIFECYCLE_ACTIONS.executeRepositoryCompaction.attrs} data-source={source}>
            <List gap="xs">
              <TextField name="backup" label="New Mirror Backup Path" required />
              <Text variant="p" flush wrap="anywhere">
                Type <code>{required}</code> to confirm this exact plan.
              </Text>
              <TextField name="confirm" label="Exact Confirmation" required />
              {state.execution?.status === 'error' && (
                <StateBanner
                  tone="danger"
                  copyLayout="stacked"
                  title="Verify the compaction outcome before retrying."
                  detail={`${state.execution.message} Check the remote head and mirror backup. If the store needs recovery, preserve local work, restore the checkout to origin, then use the CLI --recover command.`}
                />
              )}
              <Button type="submit" tone="danger" disabled={state.execution?.status === 'running'}>
                {state.execution?.status === 'running' ? 'Compacting…' : 'Back Up and Rewrite History'}
              </Button>
            </List>
          </form>
        )}
        <Text variant="p" tone="quiet" flush>
          Headless path: <code>{command}</code>.
        </Text>
      </List>
    </section>
  );
}

/** Join display names into readable prose: `a`, `a and b`, `a, b, and c`. */
function namesList(names: string[]): string {
  if (names.length <= 2) return names.join(' and ');
  return `${names.slice(0, -1).join(', ')}, and ${names.at(-1) ?? ''}`;
}

/**
 * The repositories at the report's highest level, as readable prose with its verb, for
 * banners naming what is over a GitHub limit (HS2-QF44T7, HS2-P1GYWH).
 */
export function limitSubject(report: RepositorySizeReport): { subject: string; verb: 'is' | 'are' } {
  const over = report.entries.filter((entry) => entry.level === report.level).map(repositorySizeEntryTitle);
  return { subject: namesList(over), verb: over.length === 1 ? 'is' : 'are' };
}

/** The banner naming the repositories over GitHub's limits; none while every entry is ok. */
function limitBanner(report: RepositorySizeReport) {
  if (report.level === 'ok') return;
  const { subject, verb } = limitSubject(report);
  if (report.level === 'critical')
    return (
      <StateBanner
        tone="danger"
        urgency="alert"
        copyLayout="stacked"
        title={`${subject} ${verb} over GitHub's ${formatBytes(report.thresholds.critical_bytes)} hard limit.`}
        detail="GitHub rejects pushes to repositories this large. Shrink the repository, usually by removing large attachments from its history, before the next push."
        icon={<LucideIcon icon={OctagonAlert} name="octagon-alert" />}
      />
    );
  return (
    <StateBanner
      tone="warning"
      copyLayout="stacked"
      title={`${subject} ${verb} over GitHub's ${formatBytes(report.thresholds.warning_bytes)} soft limit.`}
      detail={`GitHub recommends staying under ${formatBytes(report.thresholds.warning_bytes)} and rejects repositories over ${formatBytes(report.thresholds.critical_bytes)}. Large ticket attachments are the usual cause.`}
      icon={<LucideIcon icon={TriangleAlert} name="triangle-alert" />}
    />
  );
}

/**
 * Repository size for one project (HS2-QF44T7): git object size (what a push to GitHub
 * carries) for the code repository and each linked ticket store, plus each store's
 * attachment payload. The server computes each entry's level against GitHub's limits; this
 * component only presents it.
 */
export function RepositorySize({
  state,
  compaction = { status: 'idle' },
}: {
  state: RepositorySizeState;
  compaction?: CompactionPreviewState;
}) {
  if (state.status === 'loading')
    return (
      <div class="repository-size" data-component="repository-size" data-state="loading">
        <EmptyState busy title="Loading Repository Size" />
      </div>
    );
  if (state.status === 'error')
    return (
      <div class="repository-size" data-component="repository-size" data-state="error">
        <StateBanner
          title="Repository size is unavailable."
          detail={state.message}
          tone="danger"
          urgency="alert"
          copyLayout="stacked"
          icon={<LucideIcon icon={CircleAlert} name="circle-alert" />}
        />
      </div>
    );
  const { report } = state;
  return (
    <section
      class="repository-size"
      data-component="repository-size"
      data-state="ready"
      data-level={report.level}
      aria-labelledby="repository-size-title"
    >
      <List gap="l">
        <List gap="xs">
          <Text variant="h2" size="large" flush id="repository-size-title">
            Repository size
          </Text>
          <Text variant="p" tone="quiet" flush>
            Git object size is what a push to GitHub carries. GitHub recommends staying under{' '}
            {formatBytes(report.thresholds.warning_bytes)} and rejects repositories over{' '}
            {formatBytes(report.thresholds.critical_bytes)}.
          </Text>
        </List>
        {limitBanner(report)}
        {report.entries.length === 0 ? (
          <EmptyState
            title="Nothing to Measure"
            detail="This project has no repository or ticket source yet."
            icon={<LucideIcon icon={HardDrive} name="hard-drive" />}
          />
        ) : (
          report.entries.map((entry) => <RepositoryEntry entry={entry} />)
        )}
        <CompactionPreview state={compaction} checkout={report.checkout_id} />
      </List>
    </section>
  );
}
