import { expect, it } from 'vitest';

import { HaltedSessionPopup } from './halted-session-popup';

it('composes an accessible permission-style halt card with the production actions', () => {
  const markup = String(
    HaltedSessionPopup({
      episode: {
        key: 'episode',
        kind: 'terminal',
        projectId: 'p',
        projectName: '<Project>',
        sessionId: 't',
        sessionName: 'Claude',
        message: '<script>bad</script>',
      },
    }),
  );
  expect(markup).toContain('popover="manual"');
  expect(markup).toContain('data-top-layer-overlay');
  expect(markup).toContain('aria-label="AI session halted"');
  expect(markup).toContain('class="halted-session-popup__card"');
  expect(markup).toContain('class="halted-session-popup__details"');
  expect(markup).toContain('class="halted-session-popup__primary"');
  expect(markup).toContain('data-action="open-halted-session"');
  expect(markup).toContain('data-action="dismiss-halted-session"');
  expect(markup).toContain('data-action="pause-notifications"');
  expect(markup).not.toContain('<script>');
});
