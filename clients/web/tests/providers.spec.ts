import {test,expect} from '@playwright/test';
test('manages connections, filters providers, and honors capabilities',async({page})=>{
  let connections=[{id:'github-main',provider:'github',locator:'acme/repo',name:'Public',default:true,settings:{credential:{secret:'github-work'}}}];
  const descriptors=[
    {connection_id:'github-main',provider:'github',display_name:'GitHub acme/repo',locator:'acme/repo',default:true,capabilities:{create:true,update:true,close:true,notes:true,attachments:false,assignment:true,review_requests:false,dependencies:false,up_next:false,close_reasons:true,claims:false,atomic_batch:false,offline_mutation:false,history:true,watch:true,provider_idempotency:false,query_fields:[]}},
    {connection_id:'jira-team',provider:'jira',display_name:'Jira ENG',locator:'ENG',default:false,capabilities:{create:true,update:true,close:false,notes:true,attachments:false,assignment:true,review_requests:false,dependencies:false,up_next:false,close_reasons:false,claims:false,atomic_batch:false,offline_mutation:false,history:true,watch:true,provider_idempotency:false,query_fields:[]}}
  ];
  await page.route('**/*',async route=>{const url=new URL(route.request().url()),path=url.pathname,method=route.request().method();if(path==='/providers')return route.fulfill({json:descriptors});if(path==='/provider-connections'&&method==='GET')return route.fulfill({json:connections});if(path==='/provider-connections'&&method==='POST'){const body=route.request().postDataJSON();connections=[...connections,body];return route.fulfill({status:201,json:body})}if(path.startsWith('/provider-connections/')&&method==='DELETE'){connections=connections.filter(c=>c.id!==path.split('/').pop());return route.fulfill({status:204})}if(path==='/providers/github-main/tickets')return route.fulfill({json:[{qualified_id:'github-main:12',native_id:'12',native_url:'https://github.test/12',title:'GitHub bug',status:'not_started',connection_id:'github-main'}]});if(path==='/providers/jira-team/tickets')return route.fulfill({json:[{qualified_id:'jira-team:ENG-9',native_id:'ENG-9',native_url:'https://jira.test/ENG-9',title:'Jira task',status:'started',connection_id:'jira-team'}]});return route.continue()});
  await page.goto('/');
  await expect(page.getByText('GitHub bug')).toBeVisible();
  await expect(page.getByRole('link',{name:/github-main:12/})).toHaveAttribute('href','https://github.test/12');
  await page.locator('[data-filter]').evaluate((node:HTMLElement&{value:string})=>{node.value='jira-team';node.dispatchEvent(new Event('change',{bubbles:true}))});
  await expect(page.getByText('Jira task')).toBeVisible();
  const jira=page.locator('.tickets article').filter({hasText:'Jira task'});await expect(jira.getByRole('button',{name:'Close'})).toBeDisabled();await expect(jira.getByRole('button',{name:'Close'})).toHaveAttribute('title','This provider does not support closing');
  await page.getByRole('button',{name:'Add connection'}).click();
  await page.locator('wa-input[name=id]').evaluate((n:HTMLElement&{value:string})=>n.value='gitlab-team');
  await page.locator('wa-select[name=provider]').evaluate((n:HTMLElement&{value:string})=>n.value='gitlab');
  await page.locator('wa-input[name=locator]').evaluate((n:HTMLElement&{value:string})=>n.value='team/project');
  await page.locator('wa-input[name=credential]').evaluate((n:HTMLElement&{value:string})=>n.value='gitlab-work');
  const posted=page.waitForRequest(request=>new URL(request.url()).pathname==='/provider-connections'&&request.method()==='POST');
  await page.getByRole('button',{name:'Save'}).click();
  await posted;
  await expect(page.getByText(/gitlab · team\/project/)).toBeVisible();
});
