/** WebKit currently clamps restored deep-list scroll positions when row content is skipped. */
export function supportsTicketRowContainment(userAgent: string): boolean {
  return /\b(?:HeadlessChrome|Chrome|Chromium|Edg)\/\d/.test(userAgent);
}
