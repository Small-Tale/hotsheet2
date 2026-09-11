import {readFileSync} from 'node:fs';

import {describe,expect,it} from 'vitest';

import type {PermissionItem} from '../permission-notifications';
import {AIConversation} from './ai-conversation';
import {PermissionRequestPopup} from './permission-request-card';

const permission:PermissionItem={id:1,connection:'connection-1',tool:'Bash',action:'npm test',key:'project:1',projectId:'project',projectName:'Project',agent:'Codex',role:'main worker',receivedAt:1,ignored:false,always_allow_supported:true};
const css=readFileSync(new URL('./ai-conversation.css',import.meta.url),'utf8');

describe('AIConversation',()=>{
  it('renders ordered Markdown turns, progress, and an inline permission request',()=>{
    const markup=String(AIConversation({open:true,tool:'Codex',sessionId:'session-1',messages:[{id:'one',role:'user',content:'Please **check** this.'},{id:'two',role:'assistant',content:'Checking `main.tsx`.',status:'streaming',usage:{tokensIn:12_000,tokensOut:840,costUsd:.0423,model:'codex-5.6'}}],draft:'Follow up',busy:true,progress:'Waiting for permission…',interruptible:true,permissions:[permission],activity:[{id:'activity-1',tool:'Codex',kind:'edit',summary:'Edited main.tsx',importance:'normal'}],totalUsage:{tokensIn:12_000,tokensOut:840,costUsd:.0423},feedbackAvailable:true}));
    expect(markup.indexOf('Please <strong>check</strong> this.')).toBeLessThan(markup.indexOf('Checking <code>main.tsx</code>.'));
    expect(markup).toContain('data-action="stop-conversation"');
    expect(markup).toContain('data-component="permission-request-card"');
    expect(markup).toContain('Waiting for permission');
    expect(markup).toContain('12.8K tokens');expect(markup).toContain('≈$0.04');expect(markup).toContain('Edited main.tsx');expect(markup).toContain('AI-generated');expect(markup).toContain('may contain errors');
    expect(markup).toContain('aria-label="AI-generated response by Codex"');
    expect(markup).toContain('data-ai-feedback-target="activity:activity-1"');
    expect(markup).toContain('without-header');
    expect(markup).toContain('light-dismiss');
    expect(markup).not.toContain('data-action="close-conversation"');
    expect(markup).toContain('data-component="dialog-header"');
    expect(markup).toContain('Enter to send · Shift+Enter for a new line');
    expect(markup).toContain('appearance="accent"');
    expect(markup).toContain('Codex is working');
    expect(markup).not.toContain('Session session-1');
  });

  it('hides stop when interruption is unavailable and exposes terminal failures',()=>{
    const markup=String(AIConversation({open:true,tool:'Codex',messages:[{id:'one',role:'assistant',content:'The turn ended.',status:'failed'}],draft:'',busy:true,progress:'Working…',interruptible:false,error:'The tool turn failed.'}));
    expect(markup).not.toContain('data-action="stop-conversation"');
    expect(markup).toContain('data-status="failed"');
    expect(markup).toContain('Conversation unavailable');
    expect(markup).toContain('circle-alert');
    expect(markup).toContain('The tool turn failed.');
  });

  it('only offers persisted ticket feedback when the composition has a ticket note target',()=>{
    const message={id:'answer',role:'assistant' as const,content:'Done.',status:'completed' as const};
    expect(String(AIConversation({open:true,tool:'Codex',messages:[message],draft:'',busy:false,interruptible:false}))).not.toContain('rate-ai-content');
    expect(String(AIConversation({open:true,tool:'Codex',messages:[message],draft:'',busy:false,interruptible:false,feedbackAvailable:true}))).toContain('data-ai-feedback-target="conversation:answer"');
  });

  it('reuses the complete conversation surface as embedded drawer content',()=>{const markup=String(AIConversation({open:true,presentation:'embedded',tool:'Codex',messages:[],draft:'Ask about the project',busy:false,interruptible:false}));expect(markup).toContain('data-presentation="embedded"');expect(markup).toContain('ai-conversation--embedded');expect(markup).toContain('Conversation transcript');expect(markup).toContain('send-conversation-turn');expect(markup).not.toContain('wa-dialog')});
  it('keeps the embedded composer fixed while the transcript owns bounded scrolling',()=>{expect(css).toMatch(/\.ai-conversation--embedded \{[^}]*display: flex[^}]*height: 100%[^}]*min-height: 0[^}]*overflow: hidden[^}]*flex-direction: column/);expect(css).toMatch(/\.ai-conversation--embedded > :not\(\.ai-conversation__transcript\) \{ flex: none; \}/);expect(css).toMatch(/\.ai-conversation--embedded > \.ai-conversation__transcript \{[^}]*padding-block: var\(--wa-space-s\)[^}]*flex: 1 1 0/);expect(css).toMatch(/\.ai-conversation__transcript \{[^}]*box-sizing: border-box/)});
  it('offers exports for completed transcripts and makes partial saved transcripts read-only',()=>{
    const message={id:'answer',role:'assistant' as const,content:'Saved result.',status:'completed' as const};
    const active=String(AIConversation({open:true,presentation:'embedded',tool:'Codex',messages:[message],draft:'Continue',busy:false,interruptible:false}));
    expect(active).toContain('data-action="save-conversation"');expect(active).toContain('aria-label="Save conversation"');expect(active).toContain('send-conversation-turn');
    const saved=String(AIConversation({open:true,presentation:'embedded',tool:'Codex',messages:[message],draft:'',busy:false,interruptible:false,readOnly:true,savedSource:'/Exports/review.hotsheet-chat'}));
    expect(saved).toContain('data-read-only="true"');expect(saved).toContain('Saved transcript');expect(saved).toContain('/Exports/review.hotsheet-chat');expect(saved).not.toContain('send-conversation-turn');expect(saved).not.toContain('conversation-model');
  });
  it('promotes a permission popup into the dialog top layer without duplicating its inline card',()=>{const markup=String(AIConversation({open:true,tool:'Codex',messages:[],draft:'',busy:true,interruptible:true,permissions:[permission],foreground:PermissionRequestPopup({item:permission})}));expect(markup).toContain('ai-conversation__foreground');expect(markup.match(/data-component="permission-request-card"/g)).toHaveLength(1)});
  it('offers labeled model and effort changes only when the plugin declares support',()=>{const markup=String(AIConversation({open:true,presentation:'embedded',tool:'Codex',messages:[],draft:'',busy:false,interruptible:false,model:'gpt',effort:'high',models:[{id:'gpt',label:'GPT'}],efforts:['medium','high'],canChangeModel:true,canChangeEffort:true}));expect(markup).toContain('name="conversation-model" label="Model"');expect(markup).toContain('name="conversation-effort" label="Effort"');expect(markup).not.toContain('data-action="stop-conversation"')});
  it('keeps nested Markdown and usage legible on the loud user bubble',()=>{expect(css).toMatch(/\.ai-conversation__message--user>\.markdown-preview[^}]*color: var\(--wa-color-neutral-on-loud\)/);expect(css).toMatch(/\.ai-conversation__message--user \.ai-conversation__usage[^}]*color: color-mix/)});
});
