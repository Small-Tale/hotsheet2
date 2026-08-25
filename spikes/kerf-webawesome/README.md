# Kerf + Web Awesome integration spike

This executable spike pins the proposed Hot Sheet web-client foundation and tests
the integration in a real Chromium browser. It covers custom-element upgrade,
Kerf morph identity/focus/value preservation, delegated native and Web Awesome
events, dialog keyboard dismissal, theme tokens, and a self-contained production
bundle.

Run with Node from the repository `.nvmrc`:

```sh
npm ci
npm run typecheck
npm test
```

The event test deliberately listens for `input`, `change`, `wa-input`, and
`wa-change` and records what Web Awesome 3.11.0 actually emits. Result: form controls
emit standard host `input` and `change` events; they do not emit `wa-input` or
`wa-change`. Component lifecycle events such as `wa-show` and `wa-hide` remain
prefixed and delegate through Kerf as expected.

Validated result: the four scenarios pass in Chromium and WebKit, the production bundle makes no
external requests, and `npm audit` reports zero vulnerabilities. The current
cherry-picked bundle is approximately 197 kB JS / 102 kB CSS before compression
(53.5 kB / 14.7 kB gzip).
