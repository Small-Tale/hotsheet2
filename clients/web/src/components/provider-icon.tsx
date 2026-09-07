import './provider-icon.css';

export type ProviderIconKind='github'|'gitlab'|'jira';

export function ProviderIcon({kind}:{kind:ProviderIconKind}){
  if(kind==='github')return <svg class="provider-icon" data-provider-icon="github" viewBox="0 0 24 24" role="img" aria-label="GitHub"><path fill="currentColor" d="M12 .8a11.4 11.4 0 0 0-3.6 22.2c.6.1.8-.2.8-.6v-2.2c-3.3.7-4-1.4-4-1.4-.5-1.4-1.3-1.7-1.3-1.7-1.1-.7.1-.7.1-.7 1.2.1 1.8 1.2 1.8 1.2 1.1 1.8 2.8 1.3 3.5 1 .1-.8.4-1.3.8-1.6-2.7-.3-5.5-1.3-5.5-5.7 0-1.3.5-2.3 1.2-3.1-.1-.3-.5-1.5.1-3.1 0 0 1-.3 3.1 1.2A10.7 10.7 0 0 1 12 6.4c1.1 0 2.1.1 3.1.4 2.1-1.5 3.1-1.2 3.1-1.2.6 1.6.2 2.8.1 3.1.8.8 1.2 1.8 1.2 3.1 0 4.4-2.8 5.4-5.5 5.7.4.4.8 1.1.8 2.1v2.8c0 .4.2.7.8.6A11.4 11.4 0 0 0 12 .8Z"/></svg>;
  if(kind==='gitlab')return <svg class="provider-icon" data-provider-icon="gitlab" viewBox="0 0 24 24" role="img" aria-label="GitLab"><path fill="#e24329" d="m12 22 4.4-13.5H7.6L12 22Z"/><path fill="#fc6d26" d="M12 22 7.6 8.5H1.4L12 22Zm0 0 4.4-13.5h6.2L12 22Z"/><path fill="#fca326" d="M1.4 8.5.1 12.4c-.1.4 0 .9.4 1.2L12 22 1.4 8.5Zm21.2 0 1.3 3.9c.1.4 0 .9-.4 1.2L12 22 22.6 8.5Z"/><path fill="#e24329" d="M1.4 8.5h6.2L5 1.1c-.1-.3-.6-.3-.7 0L1.4 8.5Zm21.2 0h-6.2L19 1.1c.1-.3.6-.3.7 0l2.9 7.4Z"/></svg>;
  return <svg class="provider-icon" data-provider-icon="jira" viewBox="0 0 24 24" role="img" aria-label="Jira"><path fill="#2684ff" d="M11.9 1.2 22.8 12 11.9 22.8 1.2 12 11.9 1.2Zm0 5.1L6.2 12l5.7 5.7 5.8-5.7-5.8-5.7Z"/><path fill="#0052cc" d="m11.9 6.3 5.8 5.7-5.8 5.7c-2.3-2.3-2.3-9.1 0-11.4Z"/></svg>;
}
