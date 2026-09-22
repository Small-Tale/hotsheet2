import { acceptMigrationJob, type MigrationJob, type MigrationStart } from './migration-progress';

function validatedJob(value: unknown): MigrationJob {
  const job = value as (Partial<Omit<MigrationJob, 'progress'>> & { progress?: { version?: number } }) | null;
  if (
    !job ||
    typeof job.root !== 'string' ||
    typeof job.id !== 'string' ||
    typeof job.attempt !== 'string' ||
    !Number.isSafeInteger(job.revision) ||
    typeof job.revision !== 'number' ||
    job.revision < 0 ||
    job.progress?.version !== 1 ||
    !job.status ||
    !['running', 'succeeded', 'failed', 'interrupted'].includes(job.status)
  )
    throw new Error('Unsupported migration job response. Reconnect after updating the local bridge.');
  return job as MigrationJob;
}

/** Project-owned snapshot/revision subscriptions. No interval polling or child cancellation. */
export class MigrationJobClient {
  private jobs = new Map<string, MigrationJob>();
  private watchers = new Map<string, AbortController>();
  private completed = new Set<string>();
  private finishing = new Set<string>();
  constructor(
    private receive: (job: MigrationJob) => void,
    private finished: (job: MigrationJob) => Promise<void>,
    private disconnected: (root: string, message: string) => void,
    private request: typeof fetch = (input, init) => fetch(input, init),
  ) {}

  private async decode(response: Response): Promise<unknown> {
    const value = (await response.json()) as { error?: string };
    if (!response.ok) throw new Error(value.error ?? 'Could not reach the migration owner.');
    return value;
  }
  private accept(job: MigrationJob) {
    job = validatedJob(job);
    const current = this.jobs.get(job.root),
      accepted = acceptMigrationJob(current, job);
    if (accepted !== current) {
      this.jobs.set(job.root, accepted);
      this.receive(accepted);
    }
    if (
      accepted.status === 'succeeded' &&
      !this.completed.has(accepted.attempt) &&
      !this.finishing.has(accepted.attempt)
    ) {
      this.finishing.add(accepted.attempt);
      void this.finished(accepted)
        .then(() => {
          this.completed.add(accepted.attempt);
        })
        .catch((error: unknown) => {
          this.disconnected(job.root, error instanceof Error ? error.message : String(error));
        })
        .finally(() => this.finishing.delete(accepted.attempt));
    }
  }
  async join(root: string): Promise<MigrationJob | undefined> {
    const response = await this.request(`/__hotsheet/projects/migration-jobs?root=${encodeURIComponent(root)}`),
      value = (await this.decode(response)) as { job?: MigrationJob };
    if (value.job) {
      this.accept(value.job);
      this.watch(root);
    } else {
      this.disconnect(root);
      this.jobs.delete(root);
    }
    return value.job;
  }
  async start(input: MigrationStart): Promise<MigrationJob> {
    const response = await this.request('/__hotsheet/projects/migration-jobs', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(input),
      }),
      job = (await this.decode(response)) as MigrationJob;
    this.accept(job);
    this.watch(job.root);
    return job;
  }
  private watch(root: string) {
    this.watchers.get(root)?.abort();
    const controller = new AbortController();
    this.watchers.set(root, controller);
    void (async () => {
      while (!controller.signal.aborted) {
        const current = this.jobs.get(root);
        if (!current || current.status !== 'running') return;
        const requestedAt = Date.now();
        const response = await this.request(
            `/__hotsheet/projects/migration-jobs?root=${encodeURIComponent(root)}&after=${current.revision}`,
            { signal: controller.signal },
          ),
          value = (await this.decode(response)) as { job?: MigrationJob };
        if (this.watchers.get(root) !== controller) return;
        if (!value.job) throw new Error('The migration checkpoint is unavailable. Reopen the project to recover.');
        if (value.job.root !== root || value.job.id !== current.id || value.job.revision < current.revision)
          throw new Error(
            'The migration watch returned an outdated or different job. Reconnect to its current status.',
          );
        if (
          value.job.revision === current.revision &&
          value.job.status === 'running' &&
          Date.now() - requestedAt < 1000
        )
          throw new Error('The migration watch did not wait for changes. Reconnect after updating the local bridge.');
        this.accept(value.job);
      }
    })()
      .catch((error: unknown) => {
        if (!controller.signal.aborted) this.disconnected(root, error instanceof Error ? error.message : String(error));
      })
      .finally(() => {
        if (this.watchers.get(root) === controller) this.watchers.delete(root);
      });
  }
  disconnect(root: string) {
    this.watchers.get(root)?.abort();
    this.watchers.delete(root);
  }
}
