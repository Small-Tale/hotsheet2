import { describe, expect, it, vi } from 'vitest';

import { createHostDiskChoices } from './host-disk-choices';

describe('HS2-NHK8V8 host disk choices', () => {
  it('saves the choice, re-reads the snapshot, and clears an earlier failure', async () => {
    const setHostHealthDisk = vi.fn().mockRejectedValueOnce(new Error('offline')).mockResolvedValue({});
    const refresh = vi.fn(() => Promise.resolve());
    const choices = createHostDiskChoices(() => ({ setHostHealthDisk }), refresh);
    await choices.choose('uuid:A', true);
    expect(choices.message.value).toBe('offline');
    expect(refresh).not.toHaveBeenCalled();
    await choices.choose('uuid:A', false);
    expect(setHostHealthDisk).toHaveBeenLastCalledWith('uuid:A', false);
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(choices.message.value).toBeUndefined();
  });

  it('reports only the latest of overlapping failures', async () => {
    let rejectFirst: (reason: unknown) => void = () => undefined;
    const setHostHealthDisk = vi
      .fn()
      .mockImplementationOnce(
        () =>
          new Promise((_, reject) => {
            rejectFirst = reject;
          }),
      )
      .mockResolvedValueOnce({});
    const choices = createHostDiskChoices(
      () => ({ setHostHealthDisk }),
      () => Promise.resolve(),
    );
    const first = choices.choose('uuid:A', true);
    await choices.choose('uuid:B', true);
    rejectFirst(new Error('stale'));
    await first;
    expect(choices.message.value).toBeUndefined();
  });
});
