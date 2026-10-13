import { beforeEach, describe, expect, it, vi } from 'vitest';

import { routeFilePicker } from './file-picker-routes';

const wired = vi.hoisted(() => ({
  calls: [] as Array<{ root: unknown; onFiles: (selection: { id?: string; files: File[] }) => void }>,
  disposed: 0,
}));

vi.mock('@kerfjs/ui/wire-file-pickers', () => ({
  wireFilePickers: (root: unknown, { onFiles }: { onFiles: (selection: { id?: string; files: File[] }) => void }) => {
    wired.calls.push({ root, onFiles });
    return () => {
      wired.disposed += 1;
    };
  },
}));

describe('routeFilePicker (HS2-RB7KXZ)', () => {
  beforeEach(() => {
    wired.calls.length = 0;
    wired.disposed = 0;
  });

  it('shares one Kerf wiring per root and routes each report to its picker id once', () => {
    const root = {} as HTMLElement,
      a = vi.fn(),
      b = vi.fn(),
      file = { name: 'x' } as File;
    const disposeA = routeFilePicker(root, 'a', a),
      disposeB = routeFilePicker(root, 'b', b);
    expect(wired.calls).toHaveLength(1);
    const onFiles = wired.calls[0].onFiles;
    onFiles({ id: 'a', files: [file] });
    onFiles({ id: 'b', files: [file] });
    onFiles({ id: 'other', files: [file] });
    onFiles({ files: [file] });
    onFiles({ id: 'a', files: [] });
    expect(a.mock.calls.map((call) => call[0])).toEqual([[file]]);
    expect(b).toHaveBeenCalledTimes(1);
    // Disposing one route keeps the shared wiring; the last route removes it, and a later route rewires.
    disposeA();
    disposeA();
    expect(wired.disposed).toBe(0);
    onFiles({ id: 'a', files: [file] });
    expect(a).toHaveBeenCalledTimes(1);
    disposeB();
    expect(wired.disposed).toBe(1);
    routeFilePicker(root, 'a', a);
    expect(wired.calls).toHaveLength(2);
  });

  it('keeps a replacement route when the superseded disposer runs late', () => {
    const root = {} as HTMLElement,
      first = vi.fn(),
      second = vi.fn(),
      file = { name: 'y' } as File;
    const disposeFirst = routeFilePicker(root, 'a', first);
    routeFilePicker(root, 'a', second);
    disposeFirst();
    wired.calls[0].onFiles({ id: 'a', files: [file] });
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
    expect(wired.disposed).toBe(0);
  });
});
