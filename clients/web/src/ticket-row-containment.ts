/** Engines with working row content-visibility support. */
export function supportsTicketRowContainment(userAgent: string): boolean {
  return /\b(?:HeadlessChrome|Chrome|Chromium|Edg|AppleWebKit)\/\d/.test(userAgent);
}

/** WebKit changes scroll anchors while a contained list grows from its initial row batch. */
export function defersContainedListScrollRestore(userAgent: string): boolean {
  return /\bAppleWebKit\/\d/.test(userAgent) && !/\b(?:HeadlessChrome|Chrome|Chromium|Edg)\/\d/.test(userAgent);
}
