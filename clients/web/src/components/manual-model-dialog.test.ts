import {describe,expect,it} from 'vitest';

import {ManualModelDialog} from './manual-model-dialog';

describe('ManualModelDialog',()=>{
  it('asks for an exact provider model identifier',()=>{const markup=String(ManualModelDialog({state:{target:'drive',providerName:'Codex',value:'legacy model'}}));expect(markup).toContain('label="Other model"');expect(markup).toContain('accepted by Codex');expect(markup).toContain('name="manual-model"');expect(markup).toContain('value="legacy model"');expect(markup).toContain('data-action="submit-manual-model"')});
  it('does not mount a dormant dialog without state',()=>{expect(String(ManualModelDialog({}))).toBe('')});
});
