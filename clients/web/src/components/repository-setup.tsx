import './repository-setup.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { CircleCheck } from 'lucide';

export type RepositorySetupStep='initialize'|'remote';

export function RepositorySetup({step='initialize',busy=false,error=''}:{step?:RepositorySetupStep;busy?:boolean;error?:string}){
  if(step==='remote')return <form class="repository-setup" data-component="repository-setup" data-step="remote" data-action="connect-repository-remote">
    <div class="repository-setup__message"><span class="repository-setup__icon"><LucideIcon icon={CircleCheck} name="circle-check"/></span><div><strong>Git is ready</strong><p>Add an <code>origin</code> remote now, or skip this step and configure one later. Hot Sheet will not stage, commit, or push project files.</p></div></div>
    <wa-input name="repository-remote" type="text" label="Remote URL" placeholder="git@github.com:you/project.git" required autofocus></wa-input>
    {error&&<p class="repository-setup__error" role="alert">{error}</p>}
    <footer><wa-button appearance="plain" type="button" data-action="skip-repository-remote" disabled={busy}>Skip for now</wa-button><wa-button appearance="accent" type="submit" disabled={busy}>{busy?'Adding origin…':'Add origin'}</wa-button></footer>
  </form>;

  return <div class="repository-setup" data-component="repository-setup" data-step="initialize">
    <div class="repository-setup__message"><p>Initialize Git here to enable branch, change, and commit status. Existing project files will remain untracked; Hot Sheet will not stage or commit them.</p></div>
    {error&&<p class="repository-setup__error" role="alert">{error}</p>}
    <footer><wa-button appearance="accent" type="button" data-action="initialize-repository" disabled={busy}>{busy?'Initializing…':'Initialize Git repository'}</wa-button></footer>
  </div>;
}
