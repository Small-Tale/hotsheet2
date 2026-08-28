import { ChevronDown, Plus } from 'lucide';
import { MenuHeader } from '../components/menu-header';

export function MenuHeaderDemo() { return <section class="menu-item-demo" aria-label="MenuHeader demo"><div><MenuHeader label="Views" action="demo-add" actionLabel="Add view" actionIcon={Plus} actionIconName="plus" /></div><div><MenuHeader label="Project Commands" action="demo-toggle" actionIcon={ChevronDown} actionIconName="chevron-down" expanded toggle /></div><p>Section labels align with MenuItem icons whether the header has an action or toggles a group.</p></section>; }
