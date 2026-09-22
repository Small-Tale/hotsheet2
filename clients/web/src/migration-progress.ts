/** Counters describe only the named phase, never the entire migration. */
export interface MigrationProgress {
  version: 1;
  phase: string;
  completed?: number;
  total?: number;
  unit?: string;
  warning?: string;
  /** Local child ownership is checkpointed so a stopped bridge cannot race its orphan. */
  childPid?: number;
  transferredBytes?: number;
  bytesPerSecond?: number;
}
export interface MigrationResult {
  ticketStore: string;
  connectionId: string;
  tickets: number;
  attachments: number;
  toolsConfigured: boolean;
  stores?: string[];
}
export interface MigrationJob {
  id: string;
  attempt: string;
  revision: number;
  projectId: string;
  root: string;
  sourceIdentity: string;
  store: string;
  kind: 'import' | 'backup';
  remote?: string;
  status: 'running' | 'succeeded' | 'failed' | 'interrupted';
  progress: MigrationProgress;
  warnings: string[];
  result?: MigrationResult;
  error?: string;
  ownerPid: number;
  childPid?: number;
  updatedAt: string;
}
export interface MigrationStart {
  projectId: string;
  root: string;
  location: string;
  kind: MigrationJob['kind'];
  remote?: string;
  retryAttempt?: string;
}

const phases: Record<string, string> = {
  starting: 'Starting import',
  export_start: 'Starting export',
  discover_database: 'Measuring database files',
  copy_database: 'Copying database',
  open_database: 'Opening database copy',
  read_database: 'Reading tickets',
  export_tickets: 'Exporting tickets',
  stage_attachments: 'Staging attachments',
  write_export: 'Writing export',
  import_tickets: 'Importing tickets',
  import_attachments: 'Checking and copying this ticket’s attachments',
  import_settings: 'Importing settings',
  commit_import: 'Committing imported files',
  verify_import: 'Verifying imported files',
  commit_receipt: 'Recording completed import',
  import_complete: 'Import complete',
  configure_tools: 'Configuring AI tools',
  register_source: 'Linking ticket repository',
  push_start: 'Connecting backup',
  enumerate_objects: 'Enumerating Git objects',
  count_objects: 'Counting Git objects',
  compress_objects: 'Compressing Git objects',
  write_objects: 'Sending Git objects',
  remote_acceptance: 'Waiting for remote acceptance',
  verify_backup: 'Verifying backup',
  complete: 'Complete',
  failed: 'Needs attention',
};
export function migrationPhaseLabel(progress: MigrationProgress): string {
  return phases[progress.phase] ?? progress.phase.replaceAll('_', ' ');
}
export function migrationPercent(progress: MigrationProgress): number | undefined {
  return typeof progress.completed === 'number' && typeof progress.total === 'number' && progress.total > 0
    ? Math.min(100, Math.max(0, (progress.completed / progress.total) * 100))
    : undefined;
}
export function migrationCounter(progress: MigrationProgress): string {
  const format = (bytes: number) =>
    bytes >= 1048576
      ? `${(bytes / 1048576).toFixed(1)} MB`
      : bytes >= 1024
        ? `${(bytes / 1024).toFixed(1)} KB`
        : `${bytes} B`;
  const transfer =
    progress.transferredBytes === undefined
      ? ''
      : `${format(progress.transferredBytes)} sent${progress.bytesPerSecond === undefined ? '' : ` at ${format(progress.bytesPerSecond)}/s`}`;
  if (progress.completed === undefined || progress.total === undefined) return transfer;
  if (progress.unit === 'bytes') return `${format(progress.completed)} of ${format(progress.total)}`;
  return `${progress.completed} of ${progress.total} ${progress.unit ?? 'items'}${transfer ? ` · ${transfer}` : ''}`;
}

/** Reject stale revisions, including late terminal events from a superseded attempt. */
export function acceptMigrationJob(current: MigrationJob | undefined, incoming: MigrationJob): MigrationJob {
  return current && incoming.id === current.id && incoming.revision <= current.revision ? current : incoming;
}
