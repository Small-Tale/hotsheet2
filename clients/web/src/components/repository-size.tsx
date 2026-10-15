import './repository-size.css';

import { Badge } from '@kerfjs/ui/badge';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text } from '@kerfjs/ui/text';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';
import { CircleAlert, HardDrive, OctagonAlert, TriangleAlert } from 'lucide';

import type { RepositorySizeEntry, RepositorySizeLevel, RepositorySizeReport } from '../api';

/** What the repository size section shows (HS2-QF44T7). */
export type RepositorySizeState =
  { status: 'loading' } | { status: 'error'; message: string } | { status: 'ready'; report: RepositorySizeReport };

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
      </List>
    </section>
  );
}

/** Join display names into readable prose: `a`, `a and b`, `a, b, and c`. */
function namesList(names: string[]): string {
  if (names.length <= 2) return names.join(' and ');
  return `${names.slice(0, -1).join(', ')}, and ${names.at(-1) ?? ''}`;
}

/** The banner naming the repositories over GitHub's limits; none while every entry is ok. */
function limitBanner(report: RepositorySizeReport) {
  if (report.level === 'ok') return;
  const over = report.entries.filter((entry) => entry.level === report.level).map(repositorySizeEntryTitle);
  const subject = namesList(over),
    verb = over.length === 1 ? 'is' : 'are';
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
export function RepositorySize({ state }: { state: RepositorySizeState }) {
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
      </List>
    </section>
  );
}
