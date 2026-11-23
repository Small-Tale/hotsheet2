import { describe, expect, it } from 'vitest';

import { RemoteApprovalReminderBanner } from './remote-approval-reminder-banner';

describe('HS2-4B0KSD remote approval reminder banner', () => {
  it('names the approval, its days left, and offers Renew and Dismiss', () => {
    const markup = String(
      RemoteApprovalReminderBanner({
        reminder: { scope: 'binding-a', name: 'Studio', day: 80, daysLeft: 10, requested: false },
      }),
    );
    expect(markup).toContain('data-component="remote-approval-reminder-banner"');
    expect(markup).toContain('Renew access for Studio');
    expect(markup).toContain('expires in 10 days');
    expect(markup).toContain('data-action="renew-remote-approval"');
    expect(markup).toContain('data-action="dismiss-remote-approval-reminder"');
    expect(markup).toContain('data-remote-scope="binding-a"');
  });

  it('drops Renew once the renewal was requested', () => {
    const markup = String(
      RemoteApprovalReminderBanner({
        reminder: { scope: 'browser', name: 'this browser', day: 89, daysLeft: 1, requested: true },
      }),
    );
    expect(markup).toContain('Renewal requested');
    expect(markup).toContain('tomorrow');
    expect(markup).not.toContain('data-action="renew-remote-approval"');
  });
});
