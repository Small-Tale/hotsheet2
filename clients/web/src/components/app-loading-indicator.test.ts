import { describe, expect, it } from 'vitest';

import { AppLoadingIndicator } from './app-loading-indicator';

describe('AppLoadingIndicator', () => {
  it('shows a project load, a ticket update, and a background AI request (HS2-4C32A0)', () => {
    expect(String(AppLoadingIndicator({}))).toContain('Loading…');
    expect(String(AppLoadingIndicator({ progress: { completed: 1, total: 3 } }))).toContain('Updating tickets… 1 of 3');
    const task = String(AppLoadingIndicator({ task: 'Asking Claude to choose a group…' }));
    expect(task).toContain('Asking Claude to choose a group…');
    expect(task).toContain('Waiting for the AI');
    // A ticket update outranks the AI request's label.
    expect(
      String(AppLoadingIndicator({ progress: { completed: 0, total: 2 }, task: 'Asking Claude to choose a group…' })),
    ).toContain('Updating tickets… 0 of 2');
  });
});
