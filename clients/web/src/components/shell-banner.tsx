import './shell-banner.css';

import { List } from '@kerfjs/ui/list';
import { StateBanner, type StateBannerProps } from '@kerfjs/ui/state-banner';

export type ShellBannerTone = 'info' | 'success' | 'warning';

export interface ShellBannerProps extends Omit<StateBannerProps, 'tone' | 'urgency' | 'copyLayout' | 'className'> {
  tone: ShellBannerTone;
  /** The composing banner's `data-component` name, kept on the root for tests and inventories. */
  component: string;
}

/**
 * One application banner above the workspace (setup warnings, Codex hook review, paused notifications,
 * Hot Sheet 1 migration prompts). It composes Kerf's rounded StateBanner with stacked copy and status
 * urgency, inset from the shell edges by a Kerf List, so every application banner shares the inset
 * treatment (HS2-BF8C76, HS2-YCB15Z). The list has no bottom inset: the banner slot's own bottom gap
 * ends the stack, so stacked banners sit one inset apart rather than two.
 */
export function ShellBanner({ tone, component, ...banner }: ShellBannerProps) {
  return (
    <div class="shell-banner" data-component={component} data-tone={tone}>
      <List controlInsets="trl">
        <StateBanner {...banner} tone={tone} urgency="status" copyLayout="stacked" />
      </List>
    </div>
  );
}
