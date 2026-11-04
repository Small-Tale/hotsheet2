import { afterEach, describe, expect, it } from 'vitest';

import {
  clientUpdateAvailable,
  clientUpdateNoticeDismissed,
  clientUpdateNoticeKey,
  dismissClientUpdateNotice,
  hostClientBuild,
  noteHostClientBuild,
  showsClientUpdateNotice,
} from './client-update-notice';

afterEach(() => {
  hostClientBuild.value = undefined;
  clientUpdateNoticeDismissed.value = undefined;
});

describe('newer client notice (HS2-KPJWBK)', () => {
  it('reports an update only when both builds are known and differ', () => {
    expect(clientUpdateAvailable('bundle-a', 'bundle-b')).toBe(true);
    expect(clientUpdateAvailable('bundle-a', 'bundle-a')).toBe(false);
    // An older host announces nothing; an unstamped page knows nothing.
    expect(clientUpdateAvailable('bundle-a', undefined)).toBe(false);
    expect(clientUpdateAvailable(undefined, 'bundle-b')).toBe(false);
    expect(clientUpdateAvailable(undefined, undefined)).toBe(false);
  });

  it('records valid announcements, keeps the last one through an older host, and ignores repeats', () => {
    noteHostClientBuild('bundle-a');
    expect(hostClientBuild.value).toBe('bundle-a');
    noteHostClientBuild(undefined);
    noteHostClientBuild('not valid');
    expect(hostClientBuild.value).toBe('bundle-a');
    noteHostClientBuild('bundle-b');
    expect(hostClientBuild.value).toBe('bundle-b');
  });

  it('walks reconnect sequences: same build, newer build, dismissal, then a later build', () => {
    const loaded = 'bundle-loaded';
    // A reconnect that announces the loaded bundle raises nothing.
    noteHostClientBuild(loaded);
    expect(showsClientUpdateNotice(loaded)).toBe(false);
    noteHostClientBuild(loaded);
    expect(showsClientUpdateNotice(loaded)).toBe(false);
    // A restarted host serves a newer bundle.
    noteHostClientBuild('bundle-next');
    expect(showsClientUpdateNotice(loaded)).toBe(true);
    const stored = new Map<string, string>();
    dismissClientUpdateNotice(loaded, 'bundle-next', { setItem: (key, value) => stored.set(key, value) });
    expect([...stored.values()]).toEqual([clientUpdateNoticeKey(loaded, 'bundle-next')]);
    expect(showsClientUpdateNotice(loaded)).toBe(false);
    // The same pair on a further reconnect stays dismissed; a later build announces again.
    noteHostClientBuild('bundle-next');
    expect(showsClientUpdateNotice(loaded)).toBe(false);
    noteHostClientBuild('bundle-later');
    expect(showsClientUpdateNotice(loaded)).toBe(true);
    // A host rolled back to the loaded bundle clears the notice without a dismissal.
    noteHostClientBuild(loaded);
    expect(showsClientUpdateNotice(loaded)).toBe(false);
  });

  it('keys the dismissal on the loaded and served pair', () => {
    expect(clientUpdateNoticeKey('bundle-a', 'bundle-b')).toBe('bundle-a|bundle-b');
    expect(showsClientUpdateNotice('bundle-a', 'bundle-b', 'bundle-a|bundle-b')).toBe(false);
    expect(showsClientUpdateNotice('bundle-other', 'bundle-b', 'bundle-a|bundle-b')).toBe(true);
  });

  it('dismisses for this page when storage fails, and does nothing without both builds', () => {
    dismissClientUpdateNotice('bundle-a', 'bundle-b', {
      setItem: () => {
        throw new Error('quota');
      },
    });
    expect(clientUpdateNoticeDismissed.value).toBe('bundle-a|bundle-b');
    clientUpdateNoticeDismissed.value = undefined;
    dismissClientUpdateNotice('bundle-a', undefined, { setItem: () => undefined });
    expect(clientUpdateNoticeDismissed.value).toBeUndefined();
  });
});
