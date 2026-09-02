import { describe,expect,it } from 'vitest';

import { formatPermissionCountdown,parsePermissionAutomation,parsePermissionHistory,PermissionInbox,VisiblePermissionTimer } from './permission-notifications';

const project={id:'p',name:'Project',root:'/p',apiPath:'/p'};
const request={id:1,connection:'c',tool:'Bash',action:'npm test',always_allow_supported:true};
describe('permission notifications',()=>{
  it('reconciles pending requests and records an external decision when they disappear',()=>{const inbox=new PermissionInbox();inbox.reconcile(project,[request],[{id:'c',tool:'Claude',project:'/p',role:'main',busy:true}],10);expect(inbox.visible()?.agent).toBe('Claude');inbox.ignore('p:1');expect(inbox.visible()).toBeUndefined();expect(inbox.pending()).toHaveLength(1);inbox.reconcile(project,[],[],20);expect(inbox.history()[0]).toMatchObject({decision:'external',resolvedAt:20})});
  it('reports whether reconciliation changed renderable permission state',()=>{const inbox=new PermissionInbox();expect(inbox.reconcile(project,[],[],10)).toBe(false);expect(inbox.reconcile(project,[request],[],20)).toBe(true);expect(inbox.reconcile(project,[request],[],30)).toBe(false);expect(inbox.reconcile(project,[{...request,action:'cargo test'}],[],40)).toBe(true);expect(inbox.reconcile(project,[],[],50)).toBe(true)});
  it('records known automatic decisions separately from pending state',()=>{const inbox=new PermissionInbox();inbox.reconcile(project,[request],[],10);inbox.resolve('p:1','allow','always',true,20);expect(inbox.pending()).toHaveLength(0);expect(inbox.history()[0]).toMatchObject({decision:'allow',scope:'always',automatic:true})});
  it('counts only visible presentation time and pauses while hidden',()=>{const timer=new VisiblePermissionTimer();expect(timer.tick('p:1',15_000,0)).toBe(15_000);expect(timer.tick('p:1',15_000,5_000)).toBe(10_000);timer.hide(5_000);expect(timer.tick('p:1',15_000,50_000)).toBe(10_000);expect(timer.tick('p:1',15_000,60_000)).toBe(0)});
  it('cancels one request without affecting valid fail-closed settings',()=>{const timer=new VisiblePermissionTimer();timer.tick('p:1',15_000,0);timer.cancel('p:1',1_000);expect(timer.tick('p:1',15_000,20_000)).toBeUndefined();expect(parsePermissionAutomation({action:'allow',delayMs:15_000})).toEqual({action:'allow',delayMs:15_000});expect(parsePermissionAutomation({action:'allow',delayMs:12})).toEqual({action:'allow',delayMs:60_000});expect(formatPermissionCountdown(61_001)).toBe('1:02')});
  it('rejects malformed persisted history instead of breaking project startup',()=>{expect(parsePermissionHistory({key:'not-an-array'})).toEqual([]);expect(parsePermissionHistory([null,{key:'valid',resolvedAt:4},{key:3,resolvedAt:5}])).toEqual([{key:'valid',resolvedAt:4}])});
});
