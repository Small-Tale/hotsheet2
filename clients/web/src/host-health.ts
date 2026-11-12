import { type ResponseParser, ServerResponseShapeError } from './server-response-parsers';

/**
 * The server host's health (`GET /host-health`, HS2-DTAVQA, docs/host-health.md). Every value describes
 * the machine the **server** runs on, which may not be the device showing it.
 */
export type HostHealthLevel = 'normal' | 'warning' | 'critical';
/** `unsupported` (no reader on that platform) and `unavailable` (the reader failed) never call out. */
export type HostHealthStatus = HostHealthLevel | 'unsupported' | 'unavailable';
export type HostHealthSignalId = 'disk' | 'memory' | 'cpu' | 'power' | 'network' | 'thermal';

export interface HostHealthSignalHeader {
  status: HostHealthStatus;
  /** Stable reason code, for example `low_free_space`, `projected_full`, `on_battery`. */
  reason: string | null;
  since: string | null;
}
export interface HostHealthSignals {
  disk: HostHealthSignalHeader & {
    free_bytes: number | null;
    total_bytes: number | null;
    consumption_bytes_per_hour: number | null;
    hours_to_full: number | null;
  };
  memory: HostHealthSignalHeader & {
    pressure: HostHealthLevel | null;
    swap_used_bytes: number | null;
    swap_growth_bytes_per_hour: number | null;
  };
  cpu: HostHealthSignalHeader & {
    busy_percent: number | null;
    busy_for_seconds: number | null;
    top_process: string | null;
    top_process_percent: number | null;
    /** Absent from a server older than HS2-RBFR94. */
    history?: HostHealthCpuHistory;
  };
  power: HostHealthSignalHeader & {
    on_battery: boolean | null;
    battery_percent: number | null;
    lid_closed: boolean | null;
    sleep_prevented: boolean | null;
    idle_sleep_minutes: number | null;
  };
  network: HostHealthSignalHeader & { reachable: boolean | null; interface: string | null };
  thermal: HostHealthSignalHeader & { throttled: boolean | null; cpu_speed_limit: number | null };
}
/**
 * The last hour of CPU (HS2-RBFR94): dense, epoch-aligned buckets, oldest first, ending with the bucket
 * that holds the newest sample. A bucket without a sample is `null`.
 */
export interface HostHealthCpuHistory {
  bucket_seconds: number;
  ends_at: string | null;
  average_percent: (number | null)[];
}
export interface HostHealthSnapshot {
  host: { name: string; os: string; arch: string };
  sampled_at: string | null;
  level: HostHealthLevel;
  signals: HostHealthSignals;
}
export interface HostHealthThresholdValues {
  sample_seconds: number;
  disk_warning_free_gib: number;
  disk_critical_free_gib: number;
  disk_warning_hours_to_full: number;
  disk_critical_hours_to_full: number;
  swap_warning_growth_mib_per_hour: number;
  cpu_warning_percent: number;
  cpu_sustain_minutes: number;
  battery_critical_percent: number;
}
export type HostHealthConfiguredThresholds = { [K in keyof HostHealthThresholdValues]?: number | null };
export interface HostHealthThresholds {
  defaults: HostHealthThresholdValues;
  configured: HostHealthConfiguredThresholds;
  effective: HostHealthThresholdValues;
}

export const HOST_HEALTH_SIGNALS: readonly HostHealthSignalId[] = [
  'disk',
  'memory',
  'cpu',
  'power',
  'network',
  'thermal',
];
const LEVELS = ['normal', 'warning', 'critical'];
const STATUSES = [...LEVELS, 'unsupported', 'unavailable'];
const THRESHOLD_NAMES: ReadonlyArray<keyof HostHealthThresholdValues> = [
  'sample_seconds',
  'disk_warning_free_gib',
  'disk_critical_free_gib',
  'disk_warning_hours_to_full',
  'disk_critical_hours_to_full',
  'swap_warning_growth_mib_per_hour',
  'cpu_warning_percent',
  'cpu_sustain_minutes',
  'battery_critical_percent',
];

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);
const nullableString = (value: unknown) => value === null || typeof value === 'string';

function isCpuHistory(value: unknown): value is HostHealthCpuHistory {
  return (
    isRecord(value) &&
    typeof value.bucket_seconds === 'number' &&
    value.bucket_seconds > 0 &&
    nullableString(value.ends_at) &&
    Array.isArray(value.average_percent) &&
    value.average_percent.every((item) => item === null || typeof item === 'number')
  );
}

/** `GET /host-health`: checks every field the glance surfaces read. */
export const parseHostHealth: ResponseParser<HostHealthSnapshot> = (value) => {
  const what = 'host health';
  const fail = (): never => {
    throw new ServerResponseShapeError(what);
  };
  if (!isRecord(value) || !isRecord(value.host) || !isRecord(value.signals)) return fail();
  if (typeof value.host.name !== 'string' || typeof value.host.os !== 'string') return fail();
  if (!nullableString(value.sampled_at) || !LEVELS.includes(value.level as string)) return fail();
  for (const id of HOST_HEALTH_SIGNALS) {
    const signal = value.signals[id];
    if (!isRecord(signal) || !STATUSES.includes(signal.status as string)) return fail();
    if (!nullableString(signal.reason) || !nullableString(signal.since)) return fail();
  }
  const history = (value.signals.cpu as Record<string, unknown>).history;
  if (history !== undefined && !isCpuHistory(history)) return fail();
  return value as unknown as HostHealthSnapshot;
};

/** `GET`/`PUT /host-health/thresholds`. */
export const parseHostHealthThresholds: ResponseParser<HostHealthThresholds> = (value) => {
  const what = 'host health thresholds';
  if (!isRecord(value)) throw new ServerResponseShapeError(what);
  for (const group of ['defaults', 'effective', 'configured'] as const) {
    const values = value[group];
    if (!isRecord(values)) throw new ServerResponseShapeError(what);
    for (const name of THRESHOLD_NAMES) {
      const field = values[name];
      const ok = typeof field === 'number' || (group === 'configured' && (field === null || field === undefined));
      if (!ok) throw new ServerResponseShapeError(what);
    }
  }
  return value as unknown as HostHealthThresholds;
};

/** The signals that currently call out, worst first (the dark cockpit shows one line when empty). */
export function hostHealthCallouts(snapshot: HostHealthSnapshot | undefined): HostHealthSignalId[] {
  if (!snapshot) return [];
  const rank = (status: HostHealthStatus) => (status === 'critical' ? 2 : status === 'warning' ? 1 : 0);
  return HOST_HEALTH_SIGNALS.filter((id) => rank(snapshot.signals[id].status) > 0).sort(
    (a, b) => rank(snapshot.signals[b].status) - rank(snapshot.signals[a].status),
  );
}
