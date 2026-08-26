import { Circle } from 'lucide';
import { describe, expect, it } from 'vitest';
import { LucideIcon } from './lucide-icon';

describe('LucideIcon', () => {
  it('renders official icon nodes as decorative accessible SVG', () => {
    const html = String(LucideIcon({ icon: Circle, name: 'circle', class: 'example' }));
    expect(html).toContain('<svg class="example" data-lucide="circle" aria-hidden="true"');
    expect(html).toContain('<circle cx="12" cy="12" r="10"></circle>');
    expect(html).toContain('stroke-linejoin="round"');
  });
});
