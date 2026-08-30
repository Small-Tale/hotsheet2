import './command-navigation.css';

import { ChevronDown, Hammer, type IconNode,Send, TestTube2 } from 'lucide';

import { customizationContrastColor, resolveCustomizationColor } from './customization-palette';
import { LucideIcon } from './lucide-icon';
import { MenuHeader } from './menu-header';
import { MenuItem } from './menu-item';

export interface CommandNavigationItem { id: string; label: string; color: string; icon: 'send' | 'test' | 'build'; running?: boolean }
export interface CommandNavigationProps { label: string; commands: CommandNavigationItem[]; expanded: boolean }
const icons: Record<CommandNavigationItem['icon'], [IconNode, string]> = { send: [Send, 'send'], test: [TestTube2, 'test-tube-2'], build: [Hammer, 'hammer'] };
export function CommandNavigation({ label, commands, expanded }: CommandNavigationProps) {
  return <section class="command-navigation" data-component="command-navigation">
    <MenuHeader label={label} action="toggle-command-group" actionIcon={ChevronDown} actionIconName="chevron-down" expanded={expanded} toggle />
    {expanded && <div class="command-navigation__items">{commands.map(command => { const [icon, name] = icons[command.icon]; const color = resolveCustomizationColor(command.color); return <MenuItem action="run-command" itemId={command.id} commandColor={color} className="command-navigation__command" style={`--command-color:${color};--command-text-color:${customizationContrastColor(color)}`} pressed={Boolean(command.running)} icon={<LucideIcon icon={icon} name={name} />} label={command.running ? `Running ${command.label}` : command.label} trailing={command.running ? <i aria-hidden="true"></i> : undefined} />; })}</div>}
  </section>;
}
