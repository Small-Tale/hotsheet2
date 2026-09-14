import {describe,expect,it} from 'vitest';

import {ModelInput} from './model-input';

describe('ModelInput',()=>{
  it('keeps an arbitrary model id editable while offering catalog suggestions',()=>{const markup=String(ModelInput({name:'model',label:'Model',value:'legacy model "beta"',choices:[{id:'current',label:'Current model'}]}));expect(markup).toContain('value="legacy model &quot;beta&quot;"');expect(markup).toContain('list="model-choices"');expect(markup).toContain('<option value="current">Current model</option>');expect(markup).toContain('autocomplete="off"');});
});
