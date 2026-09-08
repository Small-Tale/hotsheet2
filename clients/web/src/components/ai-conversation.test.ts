import {describe,expect,it} from 'vitest';

import type {PermissionItem} from '../permission-notifications';
import {AIConversation} from './ai-conversation';

const permission:PermissionItem={id:1,connection:'connection-1',tool:'Bash',action:'npm test',key:'project:1',projectId:'project',projectName:'Project',agent:'Codex',role:'main worker',receivedAt:1,ignored:false,always_allow_supported:true};

describe('AIConversation',()=>{
  it('renders ordered Markdown turns, progress, and an inline permission request',()=>{
    const markup=String(AIConversation({open:true,tool:'Codex',sessionId:'session-1',messages:[{id:'one',role:'user',content:'Please **check** this.'},{id:'two',role:'assistant',content:'Checking `main.tsx`.',status:'streaming'}],draft:'Follow up',busy:true,progress:'Waiting for permission…',interruptible:true,permissions:[permission]}));
    expect(markup.indexOf('Please <strong>check</strong> this.')).toBeLessThan(markup.indexOf('Checking <code>main.tsx</code>.'));
    expect(markup).toContain('data-action="stop-conversation"');
    expect(markup).toContain('data-component="permission-request-card"');
    expect(markup).toContain('Waiting for permission');
  });

  it('hides stop when interruption is unavailable and exposes terminal failures',()=>{
    const markup=String(AIConversation({open:true,tool:'Codex',messages:[{id:'one',role:'assistant',content:'The turn ended.',status:'failed'}],draft:'',busy:true,progress:'Working…',interruptible:false,error:'The tool turn failed.'}));
    expect(markup).not.toContain('data-action="stop-conversation"');
    expect(markup).toContain('data-status="failed"');
    expect(markup).toContain('The tool turn failed.');
  });
});
