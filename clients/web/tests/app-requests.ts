/**
 * Every request except the Vite dev server's module graph (`/src/`, `/node_modules/`, `/@vite/`,
 * `/@id/`, `/@fs/`). Use it in place of a `'**\/*'` catch-all whose handler only fakes app and API
 * responses and falls back for everything else. Playwright matches a RegExp in the browser driver,
 * so the roughly 850 module requests of a cold page load no longer each round-trip through the test
 * worker, which also runs the fixture servers and proxies (HS2-15EYK1).
 */
export const appRequests = /^(?![a-z]+:\/\/[^/]+\/(?:src|node_modules|@vite|@id|@fs)\/)/;
