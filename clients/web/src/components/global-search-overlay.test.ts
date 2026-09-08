import {readFileSync} from 'node:fs';
import {resolve} from 'node:path';

import {describe,expect,it} from 'vitest';

import {GlobalSearchOverlay} from './global-search-overlay';

const result={connection_id:'github',native_id:'42',qualified_id:'github:42',id:'42',slug:'HS2-SEARCH',title:'Search referenced tickets',status:'started',up_next:true,feedback_needed:false,tags:['search'],blocked_by:[],claim_count:0,matchLabel:'Ticket reference',providerLabel:'GitHub'};
describe('GlobalSearchOverlay',()=>{
  it('composes the complete search floor from real interactive components',()=>{const markup=String(GlobalSearchOverlay({open:true,query:'HS2-SEARCH',scope:'working',filters:['up-next','needs-review'],results:[result]}));for(const component of ['global-search-overlay','active-filter-bar','filter-chip','select','menu-item'])expect(markup).toContain(`data-component="${component}"`);expect(markup).toContain('Ticket reference');expect(markup).toContain('GitHub');expect(markup).toContain('save-search-as-view')});
  it('has a responsive full-height narrow presentation and a bounded desktop dialog',()=>{const css=readFileSync(resolve(import.meta.dirname,'global-search-overlay.css'),'utf8');expect(css).toContain('width:min(48rem,100%)');expect(css).toContain('@media(max-width:38rem)');expect(css).toContain('height:100%')});
});
