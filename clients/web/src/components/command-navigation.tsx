import './command-navigation.css';

import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { ArrowLeftRight, Balloon, Bot, ChevronDown, CircleCheckBig, FileText, GitCompare, GitCompareArrows, Globe, Hammer, type IconNode,Send, SoapDispenserDroplet, SquareTerminal, TestTube2, Wand } from 'lucide';

import { lucideCatalogVersion } from '../lucide-catalog';
import { resolveCommandIcon } from './command-icon';
import { customizationContrastColor, resolveCommandColor, TRANSPARENT_CUSTOMIZATION_COLOR } from './customization-palette';

const icons = {
  send: [Send, 'send'],
  test: [TestTube2, 'test-tube-2'],
  build: [Hammer, 'hammer'],
  'file-text': [FileText, 'file-text'],
  'arrow-left-right': [ArrowLeftRight, 'arrow-left-right'],
  'soap-dispenser-droplet': [SoapDispenserDroplet, 'soap-dispenser-droplet'],
  'circle-check-big': [CircleCheckBig, 'circle-check-big'],
  balloon: [Balloon, 'balloon'],
  'git-compare': [GitCompare, 'git-compare'],
  'git-compare-arrows': [GitCompareArrows, 'git-compare-arrows'],
  wand: [Wand, 'wand'],
  globe: [Globe, 'globe'],
} as const satisfies Record<string, readonly [IconNode, string]>;

export type CommandNavigationIcon = keyof typeof icons;
export function isCommandNavigationIcon(value: string): value is CommandNavigationIcon { return Object.hasOwn(icons, value); }
/** The selectable command icons (key + Lucide node + name), for the command editor's icon picker. */
export const COMMAND_ICONS = (Object.entries(icons) as [CommandNavigationIcon, readonly [IconNode, string]][]).map(([key, [icon, name]]) => ({ key, icon, name }));
export interface CommandNavigationItem { id: string; label: string; color: string; icon: string; kind?:'program'|'shell'|'ai'; group?: string; running?: boolean; lastRun?: string }
export interface CommandNavigationProps { label: string; commands: CommandNavigationItem[]; expanded: boolean; collapsedGroups?:readonly string[] }
export function CommandNavigation({ label, commands, expanded,collapsedGroups=[] }: CommandNavigationProps) {
  const groups = commands.reduce<Map<string, CommandNavigationItem[]>>((result, command) => {
    const group = command.group?.trim() || '';
    result.set(group, [...(result.get(group) ?? []), command]);
    return result;
  }, new Map());
  return <section class="command-navigation" data-component="command-navigation" data-icon-catalog={lucideCatalogVersion.value}>
    <ListHeader label={label} action="toggle-command-group" actionIcon={<LucideIcon icon={ChevronDown} name="chevron-down"/>} expanded={expanded} toggle />
    {expanded && [...groups].map(([group, items]) => {const groupExpanded=!group||!collapsedGroups.includes(group);return <div class="command-navigation__group" data-command-group={group || undefined}>{group&&<ListHeader label={group} action="toggle-command-section" actionIcon={<LucideIcon icon={ChevronDown} name="chevron-down"/>} expanded={groupExpanded} toggle/>}{groupExpanded&&<div class="command-navigation__items">{items.map(command => { const {icon,name}=resolveCommandIcon(command.icon),type=command.kind==='shell'?{icon:SquareTerminal,name:'square-terminal',label:'Shell command'}:command.kind==='ai'?{icon:Bot,name:'bot',label:'AI command'}:undefined; const color = resolveCommandColor(command.color); const transparent = color === TRANSPARENT_CUSTOMIZATION_COLOR; const textColor = transparent ? undefined : customizationContrastColor(color); const style = transparent ? undefined : `--command-color:${color};--command-text-color:${textColor};--kui-list-item-color:${textColor};--kui-list-item-background:${color};--kui-list-item-hover-background:${color};--kui-list-item-hover-border:transparent;--kui-list-item-selected-color:${textColor};--kui-list-item-selected-background:${color};--kui-list-item-selected-border:transparent`; return <ListItem action="run-command" itemId={command.id} rootAttributes={{'data-command-color':color}} className="command-navigation__command" style={style} pressed={Boolean(command.running)} title={command.lastRun ? `Last run: ${command.lastRun}. Press and hold for output.` : 'Press and hold for command history.'} icon={<LucideIcon icon={icon} name={name} />} label={command.running ? `Running ${command.label}` : command.label} trailing={command.running ? <i aria-hidden="true"></i> : type?<span class="command-navigation__type" aria-label={type.label} title={type.label}><LucideIcon icon={type.icon} name={type.name}/></span>:undefined} />; })}</div>}</div>})}
  </section>;
}
