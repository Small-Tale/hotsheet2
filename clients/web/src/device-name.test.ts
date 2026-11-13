import { describe, expect, it } from 'vitest';

import { browserSelfDescription, defaultDeviceName } from './device-name';

// Synthetic, representative user-agent strings; no real device or person.
const UA = {
  iPhoneSafari:
    'Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Mobile/15E148 Safari/604.1',
  iPhoneChrome:
    'Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/131.0 Mobile/15E148 Safari/604.1',
  macSafari:
    'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Safari/605.1.15',
  macChrome:
    'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36',
  macFirefox: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:132.0) Gecko/20100101 Firefox/132.0',
  windowsEdge:
    'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0',
  androidPhone:
    'Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36',
  androidTablet:
    'Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36',
  linuxFirefox: 'Mozilla/5.0 (X11; Linux x86_64; rv:132.0) Gecko/20100101 Firefox/132.0',
  chromebook:
    'Mozilla/5.0 (X11; CrOS x86_64 14541.0.0) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36',
};

describe('HS2-DXM0YN default device name', () => {
  it.each([
    [{ userAgent: UA.iPhoneSafari }, 'Safari on iPhone'],
    [{ userAgent: UA.iPhoneChrome }, 'Chrome on iPhone'],
    [{ userAgent: UA.macSafari }, 'Safari on macOS'],
    // iPadOS Safari presents a Mac user agent; its touch screen tells them apart.
    [{ userAgent: UA.macSafari, maxTouchPoints: 5 }, 'Safari on iPad'],
    [{ userAgent: UA.macChrome, platform: 'macOS', mobile: false, brands: ['Google Chrome'] }, 'Chrome on macOS'],
    [{ userAgent: UA.macFirefox }, 'Firefox on macOS'],
    [{ userAgent: UA.windowsEdge, platform: 'Windows', brands: ['Microsoft Edge', 'Chromium'] }, 'Edge on Windows'],
    [{ userAgent: UA.androidPhone }, 'Chrome on Android phone'],
    [{ userAgent: UA.androidTablet }, 'Chrome on Android tablet'],
    // A reduced user agent still names the device through client hints.
    [{ userAgent: UA.androidTablet, platform: 'Android', mobile: true }, 'Chrome on Android phone'],
    [{ userAgent: UA.linuxFirefox }, 'Firefox on Linux'],
    [{ userAgent: UA.chromebook }, 'Chrome on ChromeOS'],
  ])('names %o as %s', (self, name) => {
    expect(defaultDeviceName(self)).toBe(name);
  });

  it('falls back to what it knows, then to a generic name', () => {
    expect(defaultDeviceName({ userAgent: 'SyntheticAgent/1.0', platform: 'Fuchsia' })).toBe('Fuchsia');
    expect(defaultDeviceName({ userAgent: 'Mozilla/5.0 Firefox/132.0' })).toBe('Firefox');
    expect(defaultDeviceName({ userAgent: '' })).toBe('This browser');
  });

  it('reads only the user agent, low-entropy client hints, and touch points from navigator', () => {
    expect(
      browserSelfDescription({
        userAgent: UA.macChrome,
        maxTouchPoints: 0,
        userAgentData: { platform: 'macOS', mobile: false, brands: [{ brand: 'Google Chrome' }] },
      }),
    ).toEqual({
      userAgent: UA.macChrome,
      platform: 'macOS',
      mobile: false,
      brands: ['Google Chrome'],
      maxTouchPoints: 0,
    });
    expect(browserSelfDescription({ userAgent: UA.macSafari })).toEqual({ userAgent: UA.macSafari });
  });
});
