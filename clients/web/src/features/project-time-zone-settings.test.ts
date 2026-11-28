import { describe, expect, it, vi } from 'vitest';

import type { TimeZoneSettings } from '../api';
import { createProjectTimeZoneSettings } from './project-time-zone-settings';

interface Pending {
  resolve: (value: TimeZoneSettings) => void;
  reject: (reason: Error) => void;
}

/** A client whose answers the test releases in any order. */
function deferredClient() {
  const reads: Pending[] = [],
    saves: (Pending & { zone: string })[] = [];
  return {
    reads,
    saves,
    client: {
      timeZoneSettings: vi.fn(
        () => new Promise<TimeZoneSettings>((resolve, reject) => reads.push({ resolve, reject })),
      ),
      saveTimeZoneSettings: vi.fn(
        (_projectId: string, zone: string) =>
          new Promise<TimeZoneSettings>((resolve, reject) => saves.push({ resolve, reject, zone })),
      ),
    },
  };
}

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('project time zone settings controller (HS2-SZF152)', () => {
  it('loads the zone in effect, then a choice shows at once and saves', async () => {
    const { client, reads, saves } = deferredClient();
    const settings = createProjectTimeZoneSettings({ client: () => client });
    expect(settings.view('p')).toMatchObject({ timeZone: undefined, loading: true });
    const loaded = settings.refresh('p');
    expect(settings.view('p').loading).toBe(true);
    reads[0].resolve({ time_zone: 'UTC' });
    await loaded;
    expect(settings.view('p')).toEqual({ timeZone: 'UTC', loading: false, message: '', diagnostic: '' });

    const saved = settings.save('p', 'Europe/Berlin');
    expect(settings.view('p').timeZone).toBe('Europe/Berlin');
    expect(client.saveTimeZoneSettings).toHaveBeenCalledWith('p', 'Europe/Berlin');
    saves[0].resolve({ time_zone: 'Europe/Berlin' });
    await saved;
    expect(settings.view('p')).toMatchObject({ timeZone: 'Europe/Berlin', message: '' });

    // Choosing the zone already in effect writes nothing.
    await settings.save('p', 'Europe/Berlin');
    expect(client.saveTimeZoneSettings).toHaveBeenCalledTimes(1);
  });

  it('restores the previous zone and names a refusal, then a later edit clears it', async () => {
    const { client, reads, saves } = deferredClient();
    const settings = createProjectTimeZoneSettings({ client: () => client });
    const loaded = settings.refresh('p');
    reads[0].resolve({ time_zone: 'Asia/Tokyo' });
    await loaded;
    const refused = settings.save('p', 'Etc/Unknown');
    saves[0].reject(new Error('"Etc/Unknown" is not a known time zone.'));
    await refused;
    expect(settings.view('p')).toMatchObject({
      timeZone: 'Asia/Tokyo',
      message: '"Etc/Unknown" is not a known time zone.',
    });
    // Re-choosing the same zone after a refusal retries, and success clears the message.
    const retried = settings.save('p', 'Asia/Tokyo');
    expect(settings.view('p').message).toBe('');
    saves[1].resolve({ time_zone: 'Asia/Tokyo' });
    await retried;
    expect(settings.view('p')).toMatchObject({ timeZone: 'Asia/Tokyo', message: '' });
  });

  it('applies only the newest answer when saves and reads resolve out of order', async () => {
    const { client, reads, saves } = deferredClient();
    const settings = createProjectTimeZoneSettings({ client: () => client });
    const loaded = settings.refresh('p');
    reads[0].resolve({ time_zone: 'UTC' });
    await loaded;
    const first = settings.save('p', 'Europe/Berlin'),
      second = settings.save('p', 'America/New_York');
    expect(settings.view('p').timeZone).toBe('America/New_York');
    saves[1].resolve({ time_zone: 'America/New_York' });
    saves[0].resolve({ time_zone: 'Europe/Berlin' });
    await Promise.all([first, second]);
    expect(settings.view('p').timeZone).toBe('America/New_York');
    // A failure of the older save does not restore over the newer one either.
    const third = settings.save('p', 'Asia/Kolkata'),
      refresh = settings.refresh('p', true);
    saves[2].reject(new Error('lost'));
    reads[1].resolve({ time_zone: 'Asia/Kolkata' });
    await Promise.all([third, refresh]);
    expect(settings.view('p')).toMatchObject({ timeZone: 'Asia/Kolkata', message: '' });
  });

  it('names a hand-edited bad value until a zone is chosen, and keeps projects apart', async () => {
    const { client, reads, saves } = deferredClient();
    const settings = createProjectTimeZoneSettings({ client: () => client });
    const loaded = settings.refresh('a');
    reads[0].resolve({ time_zone: 'UTC', invalid_value: 'Mars', diagnostic: 'Mars is not known.' });
    await loaded;
    expect(settings.view('a').diagnostic).toBe('Mars is not known.');
    expect(settings.view('b')).toMatchObject({ timeZone: undefined, diagnostic: '' });
    // Choosing UTC itself is allowed so the bad value can be replaced.
    const saved = settings.save('a', 'UTC');
    saves[0].resolve({ time_zone: 'UTC' });
    await saved;
    expect(settings.view('a').diagnostic).toBe('');
  });

  it('reports a failed read, ignores a project that is no longer open, and resets', async () => {
    const { client, reads } = deferredClient();
    let open = true;
    const settings = createProjectTimeZoneSettings({ client: () => (open ? client : undefined) });
    const loaded = settings.refresh('p');
    reads[0].reject(new Error('offline'));
    await loaded;
    expect(settings.view('p')).toMatchObject({ message: 'offline', loading: false });
    open = false;
    await settings.refresh('p');
    await settings.save('p', 'UTC');
    expect(client.timeZoneSettings).toHaveBeenCalledTimes(1);
    expect(client.saveTimeZoneSettings).not.toHaveBeenCalled();
    settings.reset();
    expect(settings.states.value).toEqual({});
    await flush();
  });
});
