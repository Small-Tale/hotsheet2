import { appendFile, mkdir } from 'node:fs/promises';
import { homedir } from 'node:os';
import { dirname, resolve } from 'node:path';

/**
 * The dev bridge's durable log of slow interactions and requests reported by the browser's
 * development latency monitor (HS2-ZGBQ5Q). Each report is printed as a warning in the dev server
 * terminal and appended as one JSON line to the log file, so a slow moment the developer did not
 * watch happen still leaves a record an agent or person can read later.
 */
export interface DevLatencyLogEntry {
  at: string;
  kind: string;
  label: string;
  duration_ms: number;
  message: string;
  [field: string]: unknown;
}

/** Default log path: `${HOTSHEET_HOME:-~/.hotsheet2}/logs/dev-latency.jsonl`. */
export function devLatencyLogPath(environment: NodeJS.ProcessEnv = process.env): string {
  return (
    environment.HOTSHEET_DEV_LATENCY_LOG ||
    resolve(environment.HOTSHEET_HOME || resolve(homedir(), '.hotsheet2'), 'logs', 'dev-latency.jsonl')
  );
}

/** Reports kept in memory for `GET /__hotsheet/dev/latency`. */
const MEMORY_LIMIT = 200;
/** Reports accepted from one POST. */
const BATCH_LIMIT = 50;

function isEntry(value: unknown): value is DevLatencyLogEntry {
  if (!value || typeof value !== 'object') return false;
  const entry = value as Record<string, unknown>;
  return (
    typeof entry.at === 'string' &&
    typeof entry.kind === 'string' &&
    typeof entry.label === 'string' &&
    typeof entry.message === 'string' &&
    typeof entry.duration_ms === 'number' &&
    Number.isFinite(entry.duration_ms)
  );
}

export function createDevLatencyLog(
  options: {
    file?: string;
    warn?: (line: string) => void;
    append?: (file: string, text: string) => Promise<void>;
  } = {},
) {
  const file = options.file ?? devLatencyLogPath(),
    warn =
      options.warn ??
      ((line: string) => {
        console.warn(line);
      }),
    append =
      options.append ??
      (async (path: string, text: string) => {
        await mkdir(dirname(path), { recursive: true });
        await appendFile(path, text);
      });
  const recent: DevLatencyLogEntry[] = [];
  let writes = Promise.resolve();
  return {
    file,
    /** Validate and record one POSTed batch; returns how many reports were accepted. */
    async record(body: unknown): Promise<number> {
      const raw = (body as { reports?: unknown } | null)?.reports,
        page = (body as { page?: unknown } | null)?.page;
      if (!Array.isArray(raw)) throw new Error('reports must be an array');
      const entries = raw.slice(0, BATCH_LIMIT).filter(isEntry);
      for (const entry of entries) {
        const stored = typeof page === 'string' ? { ...entry, page } : entry;
        recent.push(stored);
        if (recent.length > MEMORY_LIMIT) recent.shift();
        warn(`[hotsheet dev latency] ${entry.message} — logged to ${file}`);
      }
      if (entries.length > 0) {
        const text = entries
          .map((entry) => JSON.stringify(typeof page === 'string' ? { ...entry, page } : entry))
          .join('\n');
        writes = writes
          .then(() => append(file, `${text}\n`))
          .catch((error: unknown) => {
            warn(`[hotsheet dev latency] could not write ${file}: ${String(error)}`);
          });
        await writes;
      }
      return entries.length;
    },
    /** The most recent reports, oldest first. */
    recent(): DevLatencyLogEntry[] {
      return [...recent];
    },
  };
}
