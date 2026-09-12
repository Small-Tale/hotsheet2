import { describe, expect, it } from 'vitest';

import { devServerRouteExclude } from './dev-server-routes';

describe('development server route boundary', () => {
  it.each([
    '/ux-demo',
    '/__hotsheet/folders/choose',
    '/__hotsheet/conversation-exports/open',
    '/__hotsheet/projects/open',
    '/__hotsheet/server/recover-unhealthy',
    '/__hotsheet/project-api/project/tickets',
  ])('passes %s through to the Hono bridge', path => {
    expect(devServerRouteExclude.test(path)).toBe(false);
  });

  it.each(['/', '/src/main.tsx', '/__hotsheet/not-a-route'])('leaves %s with Vite', path => {
    expect(devServerRouteExclude.test(path)).toBe(true);
  });
});
