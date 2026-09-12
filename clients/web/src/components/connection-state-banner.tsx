import './connection-state-banner.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { CloudOff, KeyRound, LoaderCircle, RefreshCw, ShieldAlert } from 'lucide';

export type ConnectionState = 'connecting' | 'reconnecting' | 'offline' | 'incompatible' | 'client-too-old' | 'server-too-old' | 'compatibility-unknown' | 'revision-mismatch' | 'authentication';
export interface ConnectionStateBannerProps { state: ConnectionState; detail?: string }
const presentation = {
  connecting: { label: 'Connecting to server', action: undefined, actionLabel: undefined, icon: LoaderCircle, iconName: 'loader-circle', tone: 'info' },
  reconnecting: { label: 'Connection interrupted', action: 'retry-connection', actionLabel: 'Retry now', icon: RefreshCw, iconName: 'refresh-cw', tone: 'warning' },
  offline: { label: 'Working from offline data', action: 'retry-connection', actionLabel: 'Reconnect', icon: CloudOff, iconName: 'cloud-off', tone: 'warning' },
  incompatible: { label: 'Server update required', action: 'show-connection-details', actionLabel: 'View details', icon: ShieldAlert, iconName: 'shield-alert', tone: 'danger' },
  'client-too-old': { label: 'Client update required', action: 'reload-client', actionLabel: 'Reload client', icon: ShieldAlert, iconName: 'shield-alert', tone: 'danger' },
  'server-too-old': { label: 'Server update required', action: 'show-connection-details', actionLabel: 'View details', icon: ShieldAlert, iconName: 'shield-alert', tone: 'danger' },
  'compatibility-unknown': { label: 'Server compatibility unknown', action: 'show-connection-details', actionLabel: 'View details', icon: ShieldAlert, iconName: 'shield-alert', tone: 'danger' },
  'revision-mismatch': { label: 'Different server build is running', action: 'show-connection-details', actionLabel: 'View details', icon: ShieldAlert, iconName: 'shield-alert', tone: 'danger' },
  authentication: { label: 'Authentication required', action: 'authenticate-connection', actionLabel: 'Sign in', icon: KeyRound, iconName: 'key-round', tone: 'danger' },
} as const;

export function ConnectionStateBanner({ state, detail }: ConnectionStateBannerProps) {
  const item = presentation[state];
  const action = item.action ? <button type="button" data-action={item.action === 'show-connection-details' ? undefined : item.action} popoverTarget={item.action === 'show-connection-details' ? 'connection-details-dialog' : undefined}>{item.actionLabel}</button> : undefined;
  return <StateBanner title={item.label} detail={detail} tone={item.tone} urgency={state === 'connecting' ? 'status' : 'alert'} className={`connection-state-banner connection-state-banner--${state}`} icon={<LucideIcon icon={item.icon} name={item.iconName} className={state === 'connecting' ? 'connection-state-banner__spinner' : undefined} />} action={action} />;
}
