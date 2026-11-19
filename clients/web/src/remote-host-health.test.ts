import { effect } from 'kerfjs';
import { describe, expect, it } from 'vitest';

import { createHostHealthResource } from './host-health-resource';
import { createRemoteHostHealth, projectHostHealth } from './remote-host-health';
import { remoteProjectId } from './remote-projects/identity';
import { demoHostHealth } from './ux-demo/live-section-fixtures';

const REMOTE = remoteProjectId('01M7S5JGSGWFWB3A2WHJ0P2NTC', 'far-ab12cd');
const OTHER = remoteProjectId('01M7S5JH74D4GDMHHFPTRD300Q', 'far-ab12cd');

const transport = (scenario: Parameters<typeof demoHostHealth>[0]) => ({
  hostHealth: () => Promise.resolve(demoHostHealth(scenario)),
});

describe('remote project host health (HS2-K4DY5G)', () => {
  it('shows a remote project its own server snapshot and a local project the local one', async () => {
    const local = createHostHealthResource(() => transport('disk-critical'));
    const remote = createRemoteHostHealth();
    await local.refresh();
    await remote.resourceFor(REMOTE, () => transport('battery')).refresh();
    const health = projectHostHealth({ value: local.state.value.snapshot }, remote);
    expect(health.forProject('demo-ab12')).toEqual(demoHostHealth('disk-critical'));
    expect(health.forProject(REMOTE)).toEqual(demoHostHealth('battery'));
  });

  it('never falls back to the local snapshot while a remote project has not reported', async () => {
    const local = createHostHealthResource(() => transport('disk-critical'));
    await local.refresh();
    const health = projectHostHealth({ value: local.state.value.snapshot }, createRemoteHostHealth());
    expect(health.forProject(REMOTE)).toBeUndefined();
  });

  it('keeps two remote projects on different servers apart', async () => {
    const remote = createRemoteHostHealth();
    await remote.resourceFor(REMOTE, () => transport('disk-critical')).refresh();
    await remote.resourceFor(OTHER, () => transport('battery')).refresh();
    expect(remote.snapshot(REMOTE)).toEqual(demoHostHealth('disk-critical'));
    expect(remote.snapshot(OTHER)).toEqual(demoHostHealth('battery'));
  });

  it('reuses one resource per project and reads only through the project transport', async () => {
    const remote = createRemoteHostHealth();
    let reads = 0;
    const counting = () => ({
      hostHealth: () => {
        reads += 1;
        return Promise.resolve(demoHostHealth('normal'));
      },
    });
    const first = remote.resourceFor(REMOTE, counting);
    expect(remote.resourceFor(REMOTE, () => transport('battery'))).toBe(first);
    await first.refresh();
    expect(reads).toBe(1);
    expect(remote.snapshot(REMOTE)).toEqual(demoHostHealth('normal'));
  });

  it('forgets a closed project and refills after a drop and after a reset', async () => {
    const remote = createRemoteHostHealth();
    await remote.resourceFor(REMOTE, () => transport('battery')).refresh();
    remote.drop(REMOTE);
    expect(remote.snapshot(REMOTE)).toBeUndefined();
    expect(() => {
      remote.drop(REMOTE);
    }).not.toThrow();
    await remote.resourceFor(REMOTE, () => transport('disk-warning')).refresh();
    expect(remote.snapshot(REMOTE)).toEqual(demoHostHealth('disk-warning'));
    remote.reset();
    expect(remote.snapshot(REMOTE)).toBeUndefined();
    expect(() => {
      remote.reset();
    }).not.toThrow();
  });

  it('stops a dropped project resource so a late read cannot repopulate it', async () => {
    const remote = createRemoteHostHealth();
    let release: (() => void) | undefined;
    const resource = remote.resourceFor(REMOTE, () => ({
      hostHealth: () =>
        new Promise((resolve) => {
          release = () => {
            resolve(demoHostHealth('battery'));
          };
        }),
    }));
    const reading = resource.refresh();
    remote.drop(REMOTE);
    release?.();
    await reading;
    expect(remote.snapshot(REMOTE)).toBeUndefined();
  });

  it('re-renders a reader that looked before the stream created the resource', async () => {
    const remote = createRemoteHostHealth();
    const seen: Array<'normal' | 'warning' | 'critical' | undefined> = [];
    const stop = effect(() => {
      seen.push(remote.snapshot(REMOTE)?.level);
    });
    await remote.resourceFor(REMOTE, () => transport('battery')).refresh();
    stop();
    expect(seen.at(0)).toBeUndefined();
    expect(seen.at(-1)).toBe('warning');
  });
});
