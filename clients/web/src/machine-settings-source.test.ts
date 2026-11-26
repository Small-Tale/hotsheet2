import { describe, expect, it } from 'vitest';

import { machineSettingsKey, machineSettingsProject } from './machine-settings-source';
import { remoteProjectId } from './remote-projects/identity';

const BINDING = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
const local = { id: 'alpha', apiPath: '/__hotsheet/project-api/alpha' },
  other = { id: 'beta', apiPath: '/__hotsheet/project-api/beta' },
  remote = {
    id: remoteProjectId(BINDING, 'two'),
    apiPath: `/__hotsheet/project-api/${remoteProjectId(BINDING, 'two')}`,
  };

describe('machine-wide settings source (HS2-N3QBVG)', () => {
  it('answers from this computer: the selected local project, else any open local one', () => {
    expect(machineSettingsProject([local, other, remote], other)).toBe(other);
    // A selected remote tab is another machine; a local project answers instead.
    expect(machineSettingsProject([remote, local], remote)).toBe(local);
    expect(machineSettingsProject([remote, local], undefined)).toBe(local);
  });

  it('falls back to the selected remote project only when no local project is open', () => {
    expect(machineSettingsProject([remote], remote)).toBe(remote);
    expect(machineSettingsProject<typeof local>([], undefined)).toBeUndefined();
  });

  it('keys a re-read by the source, not the shared bridge origin', () => {
    const origin = 'http://localhost:5173/';
    expect(machineSettingsKey(local, origin)).toBe('local:http://localhost:5173');
    expect(machineSettingsKey(other, origin)).toBe(machineSettingsKey(local, origin));
    expect(machineSettingsKey(remote, origin)).toBe(`remote:${BINDING}`);
    expect(machineSettingsKey(remote, origin)).not.toBe(machineSettingsKey(local, origin));
  });
});
