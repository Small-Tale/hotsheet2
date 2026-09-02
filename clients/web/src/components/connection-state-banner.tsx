import './connection-state-banner.css';

import { CloudOff, KeyRound, LoaderCircle, RefreshCw, ShieldAlert } from 'lucide';

import { LucideIcon } from './lucide-icon';

export type ConnectionState = 'connecting' | 'reconnecting' | 'offline' | 'incompatible' | 'client-too-old' | 'server-too-old' | 'compatibility-unknown' | 'revision-mismatch' | 'authentication';
export interface ConnectionStateBannerProps { state: ConnectionState; detail?: string }
const presentation = {
  connecting: { label: 'Connecting to server', action: undefined, actionLabel: undefined, icon: LoaderCircle, iconName: 'loader-circle' },
  reconnecting: { label: 'Connection interrupted', action: 'retry-connection', actionLabel: 'Retry now', icon: RefreshCw, iconName: 'refresh-cw' },
  offline: { label: 'Working from offline data', action: 'retry-connection', actionLabel: 'Reconnect', icon: CloudOff, iconName: 'cloud-off' },
  incompatible: { label: 'Server update required', action: 'show-connection-details', actionLabel: 'View details', icon: ShieldAlert, iconName: 'shield-alert' },
  'client-too-old': { label: 'Client update required', action: 'reload-client', actionLabel: 'Reload client', icon: ShieldAlert, iconName: 'shield-alert' },
  'server-too-old': { label: 'Server update required', action: 'show-connection-details', actionLabel: 'View details', icon: ShieldAlert, iconName: 'shield-alert' },
  'compatibility-unknown': { label: 'Server compatibility unknown', action: 'show-connection-details', actionLabel: 'View details', icon: ShieldAlert, iconName: 'shield-alert' },
  'revision-mismatch': { label: 'Different server build is running', action: 'show-connection-details', actionLabel: 'View details', icon: ShieldAlert, iconName: 'shield-alert' },
  authentication: { label: 'Authentication required', action: 'authenticate-connection', actionLabel: 'Sign in', icon: KeyRound, iconName: 'key-round' },
} as const;

export function ConnectionStateBanner({ state, detail }: ConnectionStateBannerProps) {
  const item = presentation[state];
  return <section class="connection-state-banner" data-component="connection-state-banner" data-state={state} role={state === 'connecting' ? 'status' : 'alert'}>
    <LucideIcon icon={item.icon} name={item.iconName} class={state === 'connecting' ? 'connection-state-banner__spinner' : undefined} />
    <div><strong>{item.label}</strong>{detail && <span>{detail}</span>}</div>
    {item.action && <button type="button" data-action={item.action === 'show-connection-details' ? undefined : item.action} popoverTarget={item.action === 'show-connection-details' ? 'connection-details-dialog' : undefined}>{item.actionLabel}</button>}
  </section>;
}
