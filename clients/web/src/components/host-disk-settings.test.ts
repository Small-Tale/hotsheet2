import { describe, expect, it } from 'vitest';

import { hostHealthDisksFixture } from '../ux-demo/host-health-fixtures';
import { hostDiskDetail, HostDiskSettings } from './host-disk-settings';

const row = (html: string, id: string) => {
  const start = html.indexOf(`data-host-disk="${id}"`);
  const next = html.indexOf('data-host-disk="', start + 1);
  return html.slice(start, next < 0 ? undefined : next);
};

describe('HS2-KBVZV1 HostDiskSettings', () => {
  it('describes each disk by where it is, its size, and why it is listed', () => {
    const [store, backup, scratch, camera, archive] = hostHealthDisksFixture();
    expect(hostDiskDetail(store)).toBe('Holds the ticket store; always monitored · 212 GB free of 494 GB');
    expect(hostDiskDetail(backup)).toBe('/Volumes/Backup · 14 GB free of 1863 GB');
    expect(hostDiskDetail(scratch)).toBe('/Volumes/Scratch · 410 GB free of 931 GB · new');
    expect(hostDiskDetail(camera)).toBe('/Volumes/Camera · 2 GB free of 64 GB');
    expect(hostDiskDetail(archive)).toBe('/Volumes/Archive · not connected');
    expect(hostDiskDetail({ ...archive, manual: true })).toBe('/Volumes/Archive · not connected · added by path');
  });

  it('lists every disk with its switch; the store is fixed and only absent or added disks can be forgotten', () => {
    const disks = hostHealthDisksFixture();
    const html = String(HostDiskSettings({ disks, hostName: 'studio' }));
    expect(html).toContain('on studio');
    expect((html.match(/data-host-disk="/gu) ?? []).length).toBe(disks.length);
    const store = row(html, 'store');
    expect(store).toContain('Ticket store (store)');
    expect(store).toMatch(/disabled/u);
    expect(store).not.toContain('Forget');
    expect(row(html, disks[1].id)).toMatch(/checked/u);
    expect(row(html, disks[1].id)).not.toContain('Forget');
    expect(row(html, disks[3].id)).not.toMatch(/\schecked/u);
    expect(row(html, disks[4].id)).toContain('Forget');
    // Adding is a dialog, not an inline form (Kerf interaction conventions).
    expect(html).toContain('data-action="open-add-host-disk"');
    expect(html).not.toContain('name="host-disk-path"');
  });

  it('says it is loading, and shows a failure in the status line', () => {
    expect(String(HostDiskSettings({}))).toContain('Loading disks');
    expect(String(HostDiskSettings({ disks: [], message: 'no volume holds /x' }))).toContain('no volume holds /x');
  });
});
