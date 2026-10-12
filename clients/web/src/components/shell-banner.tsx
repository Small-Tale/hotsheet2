import './shell-banner.css';

import { StateBanner, type StateBannerProps } from '@kerfjs/ui/state-banner';

export type ShellBannerTone = 'info' | 'success' | 'warning';

export interface ShellBannerProps extends Omit<StateBannerProps, 'tone' | 'urgency' | 'copyLayout' | 'className'> {
  tone: ShellBannerTone;
  /** The composing banner's `data-component` name, kept on the strip root for tests and inventories. */
  component: string;
}

/**
 * One full-width status strip above the workspace (setup warnings, paused notifications, Hot Sheet 1
 * migration prompts). It composes Kerf's StateBanner with stacked copy and status urgency and owns
 * only the strip geometry, so every shell banner shares one presentation (HS2-BF8C76).
 */
export function ShellBanner({ tone, component, ...banner }: ShellBannerProps) {
  return (
    <div class="shell-banner" data-component={component} data-tone={tone}>
      <StateBanner {...banner} tone={tone} urgency="status" copyLayout="stacked" />
    </div>
  );
}
