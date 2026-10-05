import { expect, it } from 'vitest';

import { HaltedSessionPopup } from './halted-session-popup';

it('composes accessible top-layer halt copy and production actions with public Kerf primitives', () => {
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
  expect(markup).toContain('data-component="list"');
  expect(markup).toContain('data-component="row"');
  expect(markup).toContain('data-action="open-halted-session"');
  expect(markup).toContain('data-action="dismiss-halted-session"');
  expect(markup).toContain('data-action="pause-notifications"');
  expect(markup).not.toContain('<script>');
});
