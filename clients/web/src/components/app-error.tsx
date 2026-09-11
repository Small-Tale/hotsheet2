import { X } from 'lucide';

import { LucideIcon } from './lucide-icon';

export function AppError({message}:{message:string}) {
  return <div class="app-error" data-component="app-error" role="alert"><span>{message}</span><button type="button" data-action="dismiss-app-error" aria-label="Dismiss error" title="Dismiss error"><LucideIcon icon={X} name="x"/></button></div>;
}
