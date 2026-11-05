import { describe, expect, it } from 'vitest';

import { hostHealthCallouts, type HostHealthSnapshot, parseHostHealth, parseHostHealthThresholds } from './host-health';
import { ServerResponseShapeError } from './server-response-parsers';

const header = (status: string, reason: string | null = null) => ({ status, reason, since: null });

/** Mirrors the server's `GET /host-health` shape (crates/hotsheet-server/src/host_health). */
function hostHealthFixture(statuses: Partial<Record<string, string>> = {}): HostHealthSnapshot {
  const status = (id: string) => statuses[id] ?? 'normal';
  return parseHostHealth({
    host: { name: 'studio', os: 'macos', arch: 'aarch64' },
    sampled_at: '2026-11-05T12:00:00Z',
    level: Object.values(statuses).includes('critical')
      ? 'critical'
      : Object.values(statuses).includes('warning')
        ? 'warning'
        : 'normal',
    signals: {
      disk: {
        ...header(status('disk')),
        free_bytes: 1,
        total_bytes: 2,
        consumption_bytes_per_hour: null,
        hours_to_full: null,
      },
      memory: { ...header(status('memory')), pressure: 'normal', swap_used_bytes: 0, swap_growth_bytes_per_hour: null },
      cpu: {
        ...header(status('cpu')),
        busy_percent: 3,
        busy_for_seconds: null,
        top_process: 'node',
        top_process_percent: 9,
      },
      power: {
        ...header(status('power')),
        on_battery: false,
        battery_percent: null,
        lid_closed: null,
        sleep_prevented: null,
        idle_sleep_minutes: null,
      },
      network: { ...header(status('network')), reachable: true, interface: 'en0' },
      thermal: { ...header(status('thermal')), throttled: null, cpu_speed_limit: null },
    },
  });
}

describe('host health contract (HS2-DTAVQA)', () => {
  it('accepts the server snapshot, including unsupported and unavailable signals', () => {
    const snapshot = hostHealthFixture({ thermal: 'unsupported', power: 'unavailable' });
    expect(snapshot.host.name).toBe('studio');
    expect(snapshot.signals.thermal.status).toBe('unsupported');
  });

  it('rejects a drifted snapshot', () => {
    const good = hostHealthFixture();
    expect(() => parseHostHealth({ ...good, level: 'amber' })).toThrow(ServerResponseShapeError);
    expect(() => parseHostHealth({ ...good, signals: { ...good.signals, cpu: undefined } })).toThrow(
      ServerResponseShapeError,
    );
    expect(() =>
      parseHostHealth({ ...good, signals: { ...good.signals, disk: { ...good.signals.disk, status: 'red' } } }),
    ).toThrow(ServerResponseShapeError);
    expect(() => parseHostHealth({ ...good, host: { os: 'macos' } })).toThrow(ServerResponseShapeError);
    expect(() => parseHostHealth(null)).toThrow(ServerResponseShapeError);
  });

  it('parses thresholds with nullable configured values', () => {
    const values = {
      sample_seconds: 60,
      disk_warning_free_gib: 20,
      disk_critical_free_gib: 5,
      disk_warning_hours_to_full: 24,
      disk_critical_hours_to_full: 2,
      swap_warning_growth_mib_per_hour: 1024,
      cpu_warning_percent: 90,
      cpu_sustain_minutes: 10,
      battery_critical_percent: 20,
    };
    const configured = Object.fromEntries(Object.keys(values).map((key) => [key, null]));
    const parsed = parseHostHealthThresholds({
      defaults: values,
      configured: { ...configured, cpu_warning_percent: 80 },
      effective: { ...values, cpu_warning_percent: 80 },
    });
    expect(parsed.effective.cpu_warning_percent).toBe(80);
    expect(() =>
      parseHostHealthThresholds({ defaults: values, configured, effective: { ...values, cpu_sustain_minutes: null } }),
    ).toThrow(ServerResponseShapeError);
  });

  it('lists callouts worst first and none when all normal', () => {
    expect(hostHealthCallouts(undefined)).toEqual([]);
    expect(hostHealthCallouts(hostHealthFixture({ thermal: 'unsupported' }))).toEqual([]);
    expect(hostHealthCallouts(hostHealthFixture({ cpu: 'warning', disk: 'unavailable', network: 'critical' }))).toEqual(
      ['network', 'cpu'],
    );
  });
});
