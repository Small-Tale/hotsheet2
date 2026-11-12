import type { HostCpuFrame } from './host-cpu-feed';
import type {
  HostHealthDiskVolume,
  HostHealthSignalId,
  HostHealthSignals,
  HostHealthSnapshot,
  HostHealthStatus,
} from './host-health';
import { formatDuration } from './stats-right-now';

/**
 * Pure presentation of the server host's health for the Stats page's **This Mac** cards
 * (HS2-NS0AYR, docs/34-glance-surfaces.md §34.8, docs/host-health.md). Every value describes the
 * machine the server runs on. A signal the platform cannot read says so; nothing is guessed.
 */

export interface ThisMacCard {
  id: HostHealthSignalId;
  /** Unique among the cards: the signal, or `disk:<volume id>` for another monitored disk (HS2-NHK8V8). */
  key: string;
  title: string;
  status: HostHealthStatus;
  /** The large reading, or why there is none. */
  headline: string;
  /** Rate, projection, or context lines, quiet under the headline. */
  details: string[];
  /** A sparkline under the headline (the CPU's last hour, HS2-RBFR94), when there is one. */
  trend?: { values: number[]; label: string };
  /** Each logical core's busy percent from the live feed (HS2-CPFBWP), for the per-core meter. */
  cores?: number[];
}

const GIB = 1024 ** 3;
const MIB = 1024 ** 2;

const TITLES: Record<HostHealthSignalId, string> = {
  disk: 'Disk',
  memory: 'Memory pressure',
  cpu: 'CPU',
  power: 'Power and sleep',
  network: 'Network',
  thermal: 'Thermal',
};

/** Bytes as GB or MB with one decimal under ten. */
export function formatBytes(bytes: number): string {
  const abs = Math.abs(bytes);
  if (abs >= GIB) {
    const value = abs / GIB;
    return `${value < 10 ? value.toFixed(1).replace(/\.0$/u, '') : Math.round(value)} GB`;
  }
  const value = abs / MIB;
  return `${value < 10 ? value.toFixed(1).replace(/\.0$/u, '') : Math.round(value)} MB`;
}

function hours(value: number): string {
  return formatDuration(value * 3_600_000);
}

const capitalize = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);

function nonAlerting(id: HostHealthSignalId, status: HostHealthStatus): ThisMacCard | undefined {
  if (status === 'unsupported')
    return {
      id,
      key: id,
      title: TITLES[id],
      status,
      headline: 'Not supported',
      details: ["This server's platform has no reader for it"],
    };
  if (status === 'unavailable')
    return {
      id,
      key: id,
      title: TITLES[id],
      status,
      headline: 'Unavailable',
      details: ['The last sample could not be read'],
    };
  return undefined;
}

type DiskValues = Pick<
  HostHealthDiskVolume,
  'free_bytes' | 'total_bytes' | 'consumption_bytes_per_hour' | 'hours_to_full'
> & { status: HostHealthStatus };

function diskCard(disk: DiskValues, volume?: HostHealthDiskVolume): ThisMacCard {
  const details: string[] = [];
  if (disk.consumption_bytes_per_hour !== null && disk.consumption_bytes_per_hour !== 0) {
    const sign = disk.consumption_bytes_per_hour > 0 ? '−' : '+';
    const rate = `${sign}${formatBytes(disk.consumption_bytes_per_hour)}/h`;
    details.push(disk.hours_to_full === null ? rate : `${rate} · full in ~${hours(disk.hours_to_full)}`);
  } else if (disk.consumption_bytes_per_hour === 0) details.push('steady');
  if (disk.total_bytes !== null) details.push(`of ${formatBytes(disk.total_bytes)}`);
  return {
    id: 'disk',
    key: volume ? `disk:${volume.id}` : 'disk',
    title: volume ? `${TITLES.disk} · ${volume.label}` : TITLES.disk,
    status: disk.status,
    headline: disk.free_bytes === null ? 'Unknown' : `${formatBytes(disk.free_bytes)} free`,
    details,
  };
}

function memoryCard(memory: HostHealthSignals['memory']): ThisMacCard {
  const details: string[] = [];
  if (memory.swap_used_bytes !== null) {
    const growth = memory.swap_growth_bytes_per_hour;
    const trend =
      growth === null
        ? undefined
        : Math.abs(growth) < MIB
          ? 'steady'
          : `${growth > 0 ? '+' : '−'}${formatBytes(growth)}/h`;
    details.push([`swap ${formatBytes(memory.swap_used_bytes)}`, trend].filter(Boolean).join(' · '));
  }
  return {
    id: 'memory',
    key: 'memory',
    title: TITLES.memory,
    status: memory.status,
    headline: memory.pressure === null ? 'Unknown' : capitalize(memory.pressure),
    details,
  };
}

/**
 * The CPU sparkline (HS2-RBFR94): the server's five-minute averages from the first sampled bucket to the
 * newest. Buckets before the first sample are dropped rather than drawn as idle; a later gap (the server
 * was down) reads 0 and the label names the gap. Fewer than two samples draw nothing.
 */
export function cpuTrend(history: HostHealthSignals['cpu']['history']): ThisMacCard['trend'] {
  if (!history) return undefined;
  const first = history.average_percent.findIndex((value) => value !== null);
  if (first < 0) return undefined;
  const series = history.average_percent.slice(first);
  if (series.filter((value) => value !== null).length < 2) return undefined;
  const minutes = Math.round(history.bucket_seconds / 60);
  const gaps = series.some((value) => value === null);
  return {
    values: series.map((value) => Math.round(value ?? 0)),
    label: `CPU busy percent, ${minutes}-minute averages over the last ${formatDuration(series.length * history.bucket_seconds * 1000)}${gaps ? ', 0 where the server took no sample' : ''}`,
  };
}

/** A live frame with values (HS2-CPFBWP); anything else leaves the card on the sampled snapshot. */
const liveValues = (live: HostCpuFrame | undefined) =>
  live?.status === 'live' && live.busy_percent !== null
    ? { busy: live.busy_percent, cores: live.core_percent }
    : undefined;

function cpuCard(cpu: HostHealthSignals['cpu'], live?: HostCpuFrame): ThisMacCard {
  const details: string[] = [];
  const now = liveValues(live);
  const top =
    cpu.top_process === null
      ? undefined
      : cpu.top_process_percent === null
        ? `top: ${cpu.top_process}`
        : `top: ${cpu.top_process} (${Math.round(cpu.top_process_percent)}%)`;
  const cores = now?.cores.length ? `${now.cores.length} cores` : undefined;
  if (cores !== undefined || top !== undefined) details.push([cores, top].filter(Boolean).join(' · '));
  if ((cpu.status === 'warning' || cpu.status === 'critical') && cpu.busy_for_seconds !== null)
    details.push(`busy for ${formatDuration(cpu.busy_for_seconds * 1000)}`);
  const trend = cpuTrend(cpu.history);
  return {
    id: 'cpu',
    key: 'cpu',
    title: TITLES.cpu,
    status: cpu.status,
    headline:
      now !== undefined
        ? `${Math.round(now.busy)}%`
        : cpu.busy_percent === null
          ? 'Unknown'
          : `${Math.round(cpu.busy_percent)}%`,
    details,
    ...(trend === undefined ? {} : { trend }),
    ...(now?.cores.length ? { cores: now.cores } : {}),
  };
}

function powerHeadline(power: HostHealthSignals['power']): string {
  if (power.on_battery === true && power.lid_closed === true) return 'Lid closed on battery';
  if (power.on_battery === true)
    return power.battery_percent === null ? 'On battery' : `On battery · ${Math.round(power.battery_percent)}%`;
  if (power.sleep_prevented === true) return 'Kept awake';
  if (power.reason === 'idle_sleep_allowed') return 'May sleep when idle';
  if (power.on_battery === false) return 'On power';
  return 'Unknown';
}

function powerCard(power: HostHealthSignals['power']): ThisMacCard {
  const details = [
    power.on_battery === false ? 'on AC' : undefined,
    power.lid_closed === false ? 'lid open' : undefined,
    power.sleep_prevented === true ? 'sleep assertion held' : undefined,
    power.sleep_prevented === false && power.idle_sleep_minutes !== null && power.idle_sleep_minutes > 0
      ? `idle sleep after ${power.idle_sleep_minutes}m`
      : undefined,
  ].filter((item): item is string => item !== undefined);
  return {
    id: 'power',
    key: 'power',
    title: TITLES.power,
    status: power.status,
    headline: powerHeadline(power),
    details: details.length ? [details.join(' · ')] : [],
  };
}

function networkCard(network: HostHealthSignals['network']): ThisMacCard {
  return {
    id: 'network',
    key: 'network',
    title: TITLES.network,
    status: network.status,
    headline: network.reachable === false ? 'No network route' : 'Degraded',
    details: network.interface === null ? [] : [`interface ${network.interface}`],
  };
}

function thermalCard(thermal: HostHealthSignals['thermal']): ThisMacCard {
  return {
    id: 'thermal',
    key: 'thermal',
    title: TITLES.thermal,
    status: thermal.status,
    headline: 'Throttled',
    details: thermal.cpu_speed_limit === null ? [] : [`CPU speed limit ${thermal.cpu_speed_limit}%`],
  };
}

const alerting = (status: HostHealthStatus) => status === 'warning' || status === 'critical';

/**
 * The cards to show, in a fixed order: disk, memory, CPU, and power always (unsupported ones say
 * so); network and thermal only while degraded (§34.8). A live CPU frame (HS2-CPFBWP) replaces the
 * sampled busy percent and adds the core count and per-core values.
 */
export function thisMacCards(snapshot: HostHealthSnapshot, live?: HostCpuFrame): ThisMacCard[] {
  const { signals } = snapshot;
  // With a volume list (HS2-NHK8V8) the Disk card is the store's volume and each other monitored, connected
  // volume follows it with its own card; an older server reports only the store's.
  const store = snapshot.disks?.find((disk) => disk.store);
  const storeDisk: DiskValues = store ? { ...store, status: store.status ?? signals.disk.status } : signals.disk;
  const others = (snapshot.disks ?? []).filter(
    (disk) => !disk.store && disk.present && disk.decision === 'included' && disk.status !== null,
  );
  const cards: ThisMacCard[] = [
    nonAlerting('disk', storeDisk.status) ?? diskCard(storeDisk),
    ...others.map((disk) => diskCard({ ...disk, status: disk.status ?? 'unavailable' }, disk)),
    nonAlerting('memory', signals.memory.status) ?? memoryCard(signals.memory),
    // A live frame (HS2-CPFBWP) shows even while the sampled CPU reader is unavailable.
    (liveValues(live) === undefined ? nonAlerting('cpu', signals.cpu.status) : undefined) ?? cpuCard(signals.cpu, live),
    nonAlerting('power', signals.power.status) ?? powerCard(signals.power),
  ];
  if (alerting(signals.network.status)) cards.push(networkCard(signals.network));
  if (alerting(signals.thermal.status)) cards.push(thermalCard(signals.thermal));
  return cards;
}

const OS_LABELS: Record<string, string> = { macos: 'macOS', linux: 'Linux', windows: 'Windows' };

/** "macOS · aarch64": the server host's platform, for the group's context line. */
export function hostPlatform(snapshot: HostHealthSnapshot): string {
  return [OS_LABELS[snapshot.host.os] ?? snapshot.host.os, snapshot.host.arch].filter(Boolean).join(' · ');
}
