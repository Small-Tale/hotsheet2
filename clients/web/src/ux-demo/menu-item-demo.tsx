import { Archive, Layers3 } from 'lucide';

import { LucideIcon } from '../components/lucide-icon';
import { MenuItem } from '../components/menu-item';

export function MenuItemDemo() {
  return <section class="menu-item-demo" aria-label="MenuItem demo">
    <div><h2>Standard</h2><MenuItem action="demo-menu-item" icon={<LucideIcon icon={Archive} name="archive" />} label="Archive" trailing={<small>241</small>} /></div>
    <div><h2>Selected</h2><MenuItem action="demo-menu-item" selected icon={<LucideIcon icon={Layers3} name="layers-3" />} label="Queue" trailing={<small>12</small>} /></div>
    <p>Icons, labels, trailing values, and selection boundaries share one alignment grid.</p>
  </section>;
}
