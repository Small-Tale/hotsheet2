import {describe,expect,it} from 'vitest';

import {discardProjectSettings,projectSettingsValue,updateProjectSettingsValue} from './project-settings-state';

describe('project settings state',()=>{
  it('keeps project values independent and falls back only for unseen projects',()=>{
    const first=updateProjectSettingsValue({},'alpha','commands');
    const both=updateProjectSettingsValue(first,'beta','sources');
    expect(projectSettingsValue(both,'alpha','sources')).toBe('commands');
    expect(projectSettingsValue(both,'beta','commands')).toBe('sources');
    expect(projectSettingsValue(both,'gamma','sources')).toBe('sources');
  });

  it('discards only closed projects',()=>{
    expect(discardProjectSettings({alpha:'draft A',beta:'draft B'},new Set(['alpha']))).toEqual({beta:'draft B'});
  });
});
