import { describe, expect, it, vi } from 'vitest';

import { createHostDiskChoices } from './host-disk-choices';

const transport = () => ({
  setHostHealthDisk: vi.fn(() => Promise.resolve({})),
  addHostHealthDisk: vi.fn(() => Promise.resolve({})),
  forgetHostHealthDisk: vi.fn(() => Promise.resolve({})),
});

describe('HS2-NHK8V8 host disk choices', () => {
  it('saves the choice, re-reads the snapshot, and clears an earlier failure', async () => {
    const setHostHealthDisk = vi.fn().mockRejectedValueOnce(new Error('offline')).mockResolvedValue({});
    const refresh = vi.fn(() => Promise.resolve());
    const choices = createHostDiskChoices(() => ({ ...transport(), setHostHealthDisk }), refresh);
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
      () => ({ ...transport(), setHostHealthDisk }),
      () => Promise.resolve(),
    );
    const first = choices.choose('uuid:A', true);
    await choices.choose('uuid:B', true);
    rejectFirst(new Error('stale'));
    await first;
    expect(choices.message.value).toBeUndefined();
  });

  it('HS2-KBVZV1 forgets a disk, and the Add Disk dialog keeps the path until the disk is added', async () => {
    const api = transport();
    api.addHostHealthDisk.mockRejectedValueOnce(new Error('no volume holds /nowhere'));
    const refresh = vi.fn(() => Promise.resolve());
    const choices = createHostDiskChoices(() => api, refresh);
    expect(await choices.forget('uuid:D')).toBe(true);
    expect(api.forgetHostHealthDisk).toHaveBeenCalledWith('uuid:D');
    choices.add.open();
    expect(choices.dialog.value).toEqual({ value: '', message: '', busy: false });
    await choices.add.submit();
    expect(choices.dialog.value?.message).toBe('Enter a path on the disk.');
    expect(api.addHostHealthDisk).not.toHaveBeenCalled();
    choices.add.input(' /nowhere ');
    expect(choices.dialog.value?.message).toBe('');
    await choices.add.submit();
    expect(choices.dialog.value).toEqual({ value: ' /nowhere ', message: 'no volume holds /nowhere', busy: false });
    choices.add.input('/Volumes/Share');
    const pending = choices.add.submit();
    expect(choices.dialog.value?.busy).toBe(true);
    await choices.add.submit(); // a second submit while busy is ignored
    await pending;
    expect(api.addHostHealthDisk).toHaveBeenLastCalledWith('/Volumes/Share');
    expect(api.addHostHealthDisk).toHaveBeenCalledTimes(2);
    expect(choices.dialog.value).toBeUndefined();
    expect(refresh).toHaveBeenCalledTimes(2);
    choices.add.open();
    choices.add.cancel();
    expect(choices.dialog.value).toBeUndefined();
    choices.add.input('ignored while closed');
    expect(choices.dialog.value).toBeUndefined();
  });
});
