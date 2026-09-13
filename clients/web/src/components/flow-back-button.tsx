import './flow-back-button.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { ChevronLeft } from 'lucide';

export function FlowBackButton({action,label,disabled=false}:{action:string;label:string;disabled?:boolean}){
  return <button class="flow-back-button" type="button" data-action={action} disabled={disabled}><LucideIcon icon={ChevronLeft} name="chevron-left"/>{label}</button>;
}
