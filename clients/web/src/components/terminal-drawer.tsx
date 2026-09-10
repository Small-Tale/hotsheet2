import './terminal-drawer.css';

import type {SafeHtml} from 'kerfjs/jsx-runtime';
import { Bot, FolderOpen, Grid3X3, MessageSquare, PanelBottomClose, Plus, SquareTerminal } from 'lucide';

import {orderedDrawerTabIds} from '../drawer-tab-order';
import { terminalGridContentSize } from '../terminal-grid-layout';
import { AppTab } from './app-tab';
import { LucideIcon } from './lucide-icon';
import {MenuItem} from './menu-item';
import { TerminalDashboard,type TerminalDashboardSession,TerminalSession } from './terminal-dashboard';

export interface TerminalDrawerChatTab {id:string;name:string;tool:string;content:SafeHtml;busy?:boolean;summary?:string}
export interface TerminalDrawerProps {projectId:string;projectName:string;sessions:TerminalDashboardSession[];chatTabs?:TerminalDrawerChatTab[];tabOrder?:string[];width:number;height:number;fitAcross:number;fitHigh:number;selectedId:string;magnifiedKey?:string;loading?:boolean;message?:string;maximized?:boolean;createMenuOpen?:boolean}

export function TerminalDrawer({projectId,projectName,sessions,chatTabs=[],tabOrder=[],width,height,fitAcross,fitHigh,selectedId,magnifiedKey,loading=false,message='',maximized=false,createMenuOpen=false}:TerminalDrawerProps){
  const selected=sessions.some(session=>session.id===selectedId)||chatTabs.some(chat=>chat.id===selectedId)?selectedId:'grid',selectedSession=sessions.find(session=>session.id===selected),selectedChat=chatTabs.find(chat=>chat.id===selected),gridSize=terminalGridContentSize(width,height),sessionsById=new Map(sessions.map(session=>[session.id,session])),chatsById=new Map(chatTabs.map(chat=>[chat.id,chat])),orderedIds=orderedDrawerTabIds(sessions.map(session=>session.id),chatTabs.map(chat=>chat.id),tabOrder),gridChats=chatTabs.map(chat=>({id:chat.id,name:chat.name,tool:chat.tool,busy:chat.busy,summary:chat.summary,projectId,projectName}));
  return <section class="terminal-drawer" data-component="terminal-drawer" data-project-id={projectId} data-mode={selectedChat?'ai-chat':selected==='grid'?'grid':'dedicated'} data-maximized={String(maximized)} data-terminal-drawer-measure="true" aria-label={`${projectName} terminal drawer`}>
    <header class="terminal-drawer__rail" data-action="toggle-terminal-drawer-maximize" title={`Double-click to ${maximized?'restore':'maximize'} terminal drawer`}>
      <div class="terminal-drawer__views" role="tablist" aria-label="Terminal drawer views"><button type="button" role="tab" tabindex="0" aria-selected={String(selected==='grid')} data-action="select-drawer-item" data-item-id="grid" aria-label="Project grid"><LucideIcon icon={Grid3X3} name="grid-3x3"/></button><div class="terminal-drawer__tabs">{orderedIds.map(id=>{const session=sessionsById.get(id);if(session)return <AppTab kind="terminal" id={session.id} name={session.title??session.id} selected={selected===session.id} leading={<LucideIcon icon={SquareTerminal} name="square-terminal"/>} trailing={session.busy?<i aria-label="Busy"></i>:undefined}/>;const chat=chatsById.get(id)!;return <AppTab kind="ai-chat" id={chat.id} name={chat.name} selected={selected===chat.id} leading={<LucideIcon icon={MessageSquare} name="message-square"/>}/>})}</div></div>
      <div class="terminal-drawer__create-wrap"><button type="button" class="terminal-drawer__create" data-action="toggle-terminal-create-menu" aria-label="New drawer item" aria-expanded={String(createMenuOpen)} title="New shell or AI chat"><LucideIcon icon={Plus} name="plus"/></button>{createMenuOpen&&<div class="terminal-drawer__create-menu" role="menu" aria-label="New drawer item"><MenuItem role="menuitem" action="create-terminal-drawer-item" itemId="default-shell" icon={<LucideIcon icon={SquareTerminal} name="square-terminal"/>} label="Default shell"/><MenuItem role="menuitem" action="create-terminal-drawer-item" itemId="ai-shell" icon={<LucideIcon icon={Bot} name="bot"/>} label="AI shell"/><MenuItem role="menuitem" action="create-terminal-drawer-item" itemId="ai-chat" icon={<LucideIcon icon={MessageSquare} name="message-square"/>} label="AI chat"/><MenuItem role="menuitem" action="open-saved-conversation" icon={<LucideIcon icon={FolderOpen} name="folder-open"/>} label="Saved conversation…"/></div>}</div>
      <div class="terminal-drawer__actions"><button type="button" data-action="toggle-terminal-drawer" aria-label="Hide terminal drawer" title="Hide terminal drawer"><LucideIcon icon={PanelBottomClose} name="panel-bottom-close"/></button></div>
    </header>
    <div class="terminal-drawer__content">{selectedChat?selectedChat.content:selectedSession?<TerminalSession session={selectedSession}/>:<TerminalDashboard groups={[{projectId,projectName,sessions,chats:gridChats,itemOrder:orderedIds}]} width={gridSize.width} height={gridSize.height} fitAcross={fitAcross} fitHigh={fitHigh} grouping="flow" layoutMode="drawer" magnifiedKey={magnifiedKey} loading={loading} message={message}/>}</div>
  </section>;
}
