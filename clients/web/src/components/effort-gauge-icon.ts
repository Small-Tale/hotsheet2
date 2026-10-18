import { Gauge, type IconNode } from 'lucide';

// Keep Lucide's gauge arc and move only its needle. Unknown plugin-defined effort
// names retain the stock icon instead of implying an order we do not know.
const NEEDLE_BY_EFFORT: Record<string, string> = {
  low: 'M12 14 8 10',
  medium: 'M12 14 10 9',
  high: 'M12 14 12 8',
  xhigh: 'M12 14 14 9',
  max: 'm12 14 4-4',
};

export function effortGaugeIcon(effort: string | undefined): IconNode {
  const needle = effort && NEEDLE_BY_EFFORT[effort];
  return needle ? [['path', { d: needle }], Gauge[1]] : Gauge;
}
