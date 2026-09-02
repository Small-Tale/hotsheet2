import './command-navigation.css';

import { ChevronDown, Hammer, type IconNode,Send, TestTube2 } from 'lucide';

import { customizationContrastColor, resolveCustomizationColor } from './customization-palette';
import { LucideIcon } from './lucide-icon';
import { MenuHeader } from './menu-header';
import { MenuItem } from './menu-item';

export interface CommandNavigationItem { id: string; label: string; color: string; icon: 'send' | 'test' | 'build'; group?: string; running?: boolean; lastRun?: string }
export interface CommandNavigationProps { label: string; commands: CommandNavigationItem[]; expanded: boolean }
const icons: Record<CommandNavigationItem['icon'], [IconNode, string]> = { send: [Send, 'send'], test: [TestTube2, 'test-tube-2'], build: [Hammer, 'hammer'] };
export function CommandNavigation({ label, commands, expanded }: CommandNavigationProps) {
  const groups = commands.reduce<Map<string, CommandNavigationItem[]>>((result, command) => {
    const group = command.group?.trim() || '';
    result.set(group, [...(result.get(group) ?? []), command]);
    return result;
  }, new Map());
  return <section class="command-navigation" data-component="command-navigation">
    <MenuHeader label={label} action="toggle-command-group" actionIcon={ChevronDown} actionIconName="chevron-down" expanded={expanded} toggle />
    {expanded && [...groups].map(([group, items]) => <div class="command-navigation__group" data-command-group={group || undefined}>{group && <span class="command-navigation__group-label">{group}</span>}<div class="command-navigation__items">{items.map(command => { const [icon, name] = icons[command.icon]; const color = resolveCustomizationColor(command.color); return <MenuItem action="run-command" itemId={command.id} commandColor={color} className="command-navigation__command" style={`--command-color:${color};--command-text-color:${customizationContrastColor(color)}`} pressed={Boolean(command.running)} title={command.lastRun ? `Last run: ${command.lastRun}. Press and hold for output.` : 'Press and hold for command history.'} icon={<LucideIcon icon={icon} name={name} />} label={command.running ? `Running ${command.label}` : command.label} trailing={command.running ? <i aria-hidden="true"></i> : undefined} />; })}</div></div>)}
  </section>;
}
