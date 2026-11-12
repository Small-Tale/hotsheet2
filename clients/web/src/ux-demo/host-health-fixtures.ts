import type { HostHealthSnapshot, HostHealthStatus } from '../host-health';

/**
 * Deterministic server-host snapshots in the server's exact `GET /host-health` shape (HS2-DTAVQA)
 * for the This Mac demo and tests (HS2-NS0AYR). Synthetic host name only.
 */

const GIB = 1024 ** 3;
const header = (status: HostHealthStatus, reason: string | null = null) => ({ status, reason, since: null });

export type HostHealthScenario = 'normal' | 'crossed' | 'unsupported';

export function hostHealthFixture(scenario: HostHealthScenario = 'normal'): HostHealthSnapshot {
  if (scenario === 'unsupported')
    return {
      host: { name: 'build-box', os: 'windows', arch: 'x86_64' },
      sampled_at: '2026-11-06T10:47:00Z',
      level: 'normal',
      signals: {
        disk: {
          ...header('unsupported'),
          free_bytes: null,
          total_bytes: null,
          consumption_bytes_per_hour: null,
          hours_to_full: null,
        },
        memory: { ...header('unsupported'), pressure: null, swap_used_bytes: null, swap_growth_bytes_per_hour: null },
        cpu: {
          ...header('unsupported'),
          busy_percent: null,
          busy_for_seconds: null,
          top_process: null,
          top_process_percent: null,
        },
        power: {
          ...header('unavailable'),
          on_battery: null,
          battery_percent: null,
          lid_closed: null,
          sleep_prevented: null,
          idle_sleep_minutes: null,
        },
        network: { ...header('unsupported'), reachable: null, interface: null },
        thermal: { ...header('unsupported'), throttled: null, cpu_speed_limit: null },
      },
    };
  const crossed = scenario === 'crossed';
  return {
    host: { name: 'studio', os: 'macos', arch: 'aarch64' },
    sampled_at: '2026-11-06T10:47:00Z',
    level: crossed ? 'critical' : 'normal',
    signals: {
      disk: {
        ...(crossed ? header('warning', 'projected_full') : header('normal')),
        free_bytes: crossed ? 18 * GIB : 212 * GIB,
        total_bytes: 494 * GIB,
        consumption_bytes_per_hour: crossed ? 2.1 * GIB : 0,
        hours_to_full: crossed ? 8.6 : null,
      },
      memory: {
        ...header('normal'),
        pressure: 'normal',
        swap_used_bytes: 1.2 * GIB,
        swap_growth_bytes_per_hour: 0,
      },
      cpu: {
        ...(crossed ? header('warning', 'sustained_cpu') : header('normal')),
        busy_percent: crossed ? 94 : 34,
        busy_for_seconds: crossed ? 960 : 0,
        top_process: 'cargo',
        top_process_percent: crossed ? 410 : 88,
        history: {
          bucket_seconds: 300,
          ends_at: '2026-11-06T10:50:00Z',
          average_percent: crossed
            ? [41, 38, 52, 60, 71, 88, 93, 95, 96, 94, 95, 94]
            : [null, null, null, 12, 18, 9, 22, 41, 37, 28, 31, 34],
        },
      },
      power: {
        ...header('normal'),
        on_battery: false,
        battery_percent: 100,
        lid_closed: false,
        sleep_prevented: true,
        idle_sleep_minutes: 10,
      },
      network: crossed
        ? { ...header('critical', 'no_default_route'), reachable: false, interface: null }
        : { ...header('normal'), reachable: true, interface: 'en0' },
      thermal: { ...header('normal'), throttled: false, cpu_speed_limit: 100 },
    },
  };
}
