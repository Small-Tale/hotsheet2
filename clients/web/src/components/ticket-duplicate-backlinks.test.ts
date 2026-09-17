import { describe, expect, it } from 'vitest';

import { TicketDuplicateBacklinks,TicketDuplicateTarget } from './ticket-duplicate-backlinks';

const collision = {reference:'@project-b/git-b:two',project_id:'project-b',project_name:'Beta',connection_id:'git-b',native_id:'two',qualified_id:'git-b:two',slug:'HS2-SAME',title:'Second report'};

describe('TicketDuplicateBacklinks',()=>{
  it('presents the forward duplicate target with the same readable ticket details',()=>{const markup=String(TicketDuplicateTarget({target:{id:'@project-b/git-b:two',projectName:'Beta',slug:'HS2-SAME',title:'Canonical report'}}));expect(markup).toContain('<h2 class="kui-list-header__label">Duplicate of</h2>');expect(markup).toContain('Beta · HS2-SAME');expect(markup).toContain('Canonical report');expect(markup).toContain('aria-label="Open duplicate target HS2-SAME from Beta"')});
  it('renders project-qualified same-slug backlinks as exact shared menu actions with a count badge',()=>{const markup=String(TicketDuplicateBacklinks({backlinks:[{...collision,reference:'@project-a/git-a:one',project_id:'project-a',project_name:'Alpha',connection_id:'git-a',native_id:'one',qualified_id:'git-a:one',title:'First report'},collision]}));expect(markup).toContain('<h2 class="kui-list-header__label" aria-label="Duplicates, 2 duplicates">Duplicates</h2><span class="kui-list-header__count" aria-hidden="true">2</span>');expect(markup.match(/data-component="list-item"/g)).toHaveLength(2);expect(markup).toContain('Alpha · HS2-SAME');expect(markup).toContain('Beta · HS2-SAME');expect(markup).toContain('data-item-id="@project-b/git-b:two"');expect(markup).toContain('aria-label="Open duplicate HS2-SAME from Beta"')});
  it('keeps partial results transparent when registered projects are inaccessible',()=>{const markup=String(TicketDuplicateBacklinks({backlinks:[collision],inaccessibleProjects:['Offline project']}));expect(markup).toContain('Beta · HS2-SAME');expect(markup).toContain('Could not check Offline project for additional duplicates.');expect(markup).toContain('role="status"')});
  it('omits an empty successful lookup',()=>{
    expect(TicketDuplicateBacklinks({backlinks:[]})).toBeNull();
  });
});
