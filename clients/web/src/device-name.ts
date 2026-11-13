/** What a browser says about itself: its user agent and the low-entropy User-Agent Client Hints. No
 * other signal (screen, fonts, canvas) is read, so the name proposes a kind of device, never an
 * identity (HS2-DXM0YN). */
export interface BrowserSelfDescription {
  userAgent: string;
  /** `navigator.userAgentData.platform`, for example "macOS", "Windows" or "Android". */
  platform?: string;
  /** `navigator.userAgentData.mobile`. */
  mobile?: boolean;
  /** `navigator.userAgentData.brands` names, for example "Google Chrome" or "Microsoft Edge". */
  brands?: readonly string[];
  /** `navigator.maxTouchPoints`: iPadOS Safari reports a Mac user agent but a touch screen. */
  maxTouchPoints?: number;
}

const FALLBACK = 'This browser';

function deviceKind({ userAgent, platform, mobile, maxTouchPoints = 0 }: BrowserSelfDescription): string | undefined {
  if (/\biPhone\b/.test(userAgent)) return 'iPhone';
  if (/\biPad\b/.test(userAgent)) return 'iPad';
  if (/\biPod\b/.test(userAgent)) return 'iPod';
  const hinted = platform?.trim();
  if (hinted === 'Android' || /\bAndroid\b/.test(userAgent))
    return (mobile ?? /\bMobile\b/.test(userAgent)) ? 'Android phone' : 'Android tablet';
  if (hinted === 'macOS' || /\bMacintosh\b|\bMac OS X\b/.test(userAgent)) return maxTouchPoints > 1 ? 'iPad' : 'macOS';
  if (hinted === 'Windows' || /\bWindows\b/.test(userAgent)) return 'Windows';
  if (hinted === 'Chrome OS' || hinted === 'ChromeOS' || /\bCrOS\b/.test(userAgent)) return 'ChromeOS';
  if (hinted === 'Linux' || /\bLinux\b/.test(userAgent)) return 'Linux';
  return hinted || undefined;
}

function browserName({ userAgent, brands = [] }: BrowserSelfDescription): string | undefined {
  const named = (brand: string) => brands.some((entry) => entry === brand);
  if (named('Microsoft Edge') || /\bEdgA?\/|\bEdgiOS\//.test(userAgent)) return 'Edge';
  if (named('Opera') || /\bOPR\//.test(userAgent)) return 'Opera';
  if (/\bFirefox\/|\bFxiOS\//.test(userAgent)) return 'Firefox';
  if (named('Google Chrome') || /\bChrome\/|\bCriOS\//.test(userAgent)) return 'Chrome';
  if (/\bSafari\//.test(userAgent) && /\bVersion\//.test(userAgent)) return 'Safari';
  return undefined;
}

/** A proposed device name such as "Safari on iPhone" or "Chrome on macOS"; the person may edit it. */
export function defaultDeviceName(self: BrowserSelfDescription): string {
  const kind = deviceKind(self);
  const browser = browserName(self);
  if (kind && browser) return `${browser} on ${kind}`;
  return kind ?? browser ?? FALLBACK;
}

interface NavigatorWithHints {
  userAgent: string;
  maxTouchPoints?: number;
  userAgentData?: { platform?: string; mobile?: boolean; brands?: readonly { brand: string }[] };
}

/** Read the browser's own description from `navigator`. */
export function browserSelfDescription(source: NavigatorWithHints = navigator): BrowserSelfDescription {
  const hints = source.userAgentData;
  return {
    userAgent: source.userAgent,
    ...(hints?.platform ? { platform: hints.platform } : {}),
    ...(typeof hints?.mobile === 'boolean' ? { mobile: hints.mobile } : {}),
    ...(hints?.brands ? { brands: hints.brands.map((entry) => entry.brand) } : {}),
    ...(typeof source.maxTouchPoints === 'number' ? { maxTouchPoints: source.maxTouchPoints } : {}),
  };
}
