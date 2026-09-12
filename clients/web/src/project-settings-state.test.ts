import {describe,expect,expectTypeOf,it} from 'vitest';

import {discardProjectSettings,projectSettingsValue,updateProjectSettingsValue} from './project-settings-state';

describe('project settings state',()=>{
  it('keeps project values independent and falls back only for unseen projects',()=>{
    type Category='sources'|'commands';
    const empty:Record<string,Category>={};
    const first=updateProjectSettingsValue(empty,'alpha','commands');
    const both=updateProjectSettingsValue(first,'beta','sources');
    const alpha=projectSettingsValue(both,'alpha','sources');
    expectTypeOf(first).toEqualTypeOf<Record<string,Category>>();
    expectTypeOf(alpha).toEqualTypeOf<Category>();
    expect(alpha).toBe('commands');
    expect(projectSettingsValue(both,'beta','commands')).toBe('sources');
    expect(projectSettingsValue(both,'gamma','sources')).toBe('sources');
    // @ts-expect-error the existing record is the sole source of the accepted value union
    updateProjectSettingsValue(empty,'gamma','columns');
  });

  it('discards only closed projects',()=>{
    expect(discardProjectSettings({alpha:'draft A',beta:'draft B'},new Set(['alpha']))).toEqual({beta:'draft B'});
  });
});
