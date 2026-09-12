import './command-navigation.css';

import { ArrowLeftRight, Balloon, ChevronDown, CircleCheckBig, FileText, GitCompare, GitCompareArrows, Globe, Hammer, type IconNode,Send, SoapDispenserDroplet, TestTube2, Wand } from 'lucide';

import { customizationContrastColor, resolveCustomizationColor } from './customization-palette';
import { LucideIcon } from './lucide-icon';
import { MenuHeader } from './menu-header';
import { MenuItem } from './menu-item';

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
export interface CommandNavigationItem { id: string; label: string; color: string; icon: CommandNavigationIcon; group?: string; running?: boolean; lastRun?: string }
export interface CommandNavigationProps { label: string; commands: CommandNavigationItem[]; expanded: boolean; collapsedGroups?:readonly string[] }
export function CommandNavigation({ label, commands, expanded,collapsedGroups=[] }: CommandNavigationProps) {
  const groups = commands.reduce<Map<string, CommandNavigationItem[]>>((result, command) => {
    const group = command.group?.trim() || '';
    result.set(group, [...(result.get(group) ?? []), command]);
    return result;
  }, new Map());
  return <section class="command-navigation" data-component="command-navigation">
    <MenuHeader label={label} action="toggle-command-group" actionIcon={ChevronDown} actionIconName="chevron-down" expanded={expanded} toggle />
    {expanded && [...groups].map(([group, items]) => {const groupExpanded=!group||!collapsedGroups.includes(group);return <div class="command-navigation__group" data-command-group={group || undefined}>{group&&<MenuHeader label={group} action="toggle-command-section" actionIcon={ChevronDown} actionIconName="chevron-down" expanded={groupExpanded} toggle/>}{groupExpanded&&<div class="command-navigation__items">{items.map(command => { const [icon, name] = icons[command.icon]; const color = resolveCustomizationColor(command.color); return <MenuItem action="run-command" itemId={command.id} commandColor={color} className="command-navigation__command" style={`--command-color:${color};--command-text-color:${customizationContrastColor(color)}`} pressed={Boolean(command.running)} title={command.lastRun ? `Last run: ${command.lastRun}. Press and hold for output.` : 'Press and hold for command history.'} icon={<LucideIcon icon={icon} name={name} />} label={command.running ? `Running ${command.label}` : command.label} trailing={command.running ? <i aria-hidden="true"></i> : undefined} />; })}</div>}</div>})}
  </section>;
}
