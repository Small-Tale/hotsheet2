import type { ProviderAccount } from '../api';
import { createDebouncedAutosave } from '../debounced-autosave';
import {
  inspectorBlockedReason,
  markdownEvent,
  markdownSavedValue,
  noteDemoNotes,
  readerNotes,
  readerTitle,
} from './content-components-demo';
import { inspectorTags, inspectorTitle } from './workspace-components-demo';

/** One project's linked ticket sources and the machine's connection catalog (HS2-3SCH1K). */
export const DEMO_PROJECT_SOURCES = [
  { connectionId: 'git-demo', name: 'Hot Sheet Git', provider: 'git', locator: '/work/demo.hs2', default: false },
  {
    connectionId: 'github-main',
    name: 'Product issues',
    provider: 'github',
    locator: 'small-tale/hotsheet2',
    default: true,
    color: '#3b82f6',
  },
  {
    connectionId: 'github-docs',
    name: 'Docs issues',
    provider: 'github',
    locator: 'small-tale/hotsheet-docs',
    default: false,
    color: '#3b82f6',
    disabled: true,
    sharedWith: ['marketing-site'],
  },
];
/** Machine-wide sign-ins (HS2-SM9PM8): a shared GitHub account, an unused one, and a Jira token. */
export const DEMO_ACCOUNTS: ProviderAccount[] = [
  {
    id: 'github-app-01demo',
    provider: 'github',
    host: 'github.com',
    managed: true,
    sources: [
      {
        connection_id: 'github-main',
        name: 'Product issues',
        locator: 'small-tale/hotsheet2',
        disabled: false,
        projects: [{ id: 'demo', alias: 'hotsheet2' }],
      },
      {
        connection_id: 'github-docs',
        name: 'Docs issues',
        locator: 'small-tale/hotsheet-docs',
        disabled: true,
        projects: [
          { id: 'demo', alias: 'hotsheet2' },
          { id: 'marketing', alias: 'marketing-site' },
        ],
      },
    ],
    projects: [
      { id: 'demo', alias: 'hotsheet2' },
      { id: 'marketing', alias: 'marketing-site' },
    ],
  },
  { id: 'github-app-01unused', provider: 'github', host: '', managed: true, sources: [], projects: [] },
  {
    id: 'jira-token',
    provider: 'jira',
    host: 'acme.atlassian.net',
    base_url: 'https://acme.atlassian.net',
    identity: 'dev@acme.test',
    managed: false,
    sources: [
      {
        connection_id: 'jira-ops',
        name: 'Operations',
        locator: 'OPS',
        disabled: false,
        projects: [{ id: 'ops', alias: 'ops-runbooks' }],
      },
    ],
    projects: [{ id: 'ops', alias: 'ops-runbooks' }],
  },
];
/** The drawer demo's installed AI providers: several open the AI shell submenu (HS2-3HT4PA). */
export const TERMINAL_DRAWER_DEMO_PROVIDERS = [
  { id: 'codex', name: 'Codex' },
  { id: 'claude', name: 'Claude' },
] as const;

// Debounced-autosave fixtures for the inspector and reader editors: each "save" writes the demo state the
// next render reads, like the production autosave writes the ticket (HS2-HMVBZW).
export const markdownAutosave = createDebouncedAutosave((value: string) => {
  markdownSavedValue.value = value;
  markdownEvent.value = 'Markdown autosaved.';
  return Promise.resolve(true);
});
export const blockedReasonAutosave = createDebouncedAutosave((value: string) => {
  inspectorBlockedReason.value = value.trim();
  return Promise.resolve(true);
});
export const noteAutosave = createDebouncedAutosave(({ id, value }: { id: string; value: string }) => {
  readerNotes.value = readerNotes.value.map((note) => (note.id === id ? { ...note, body: value } : note));
  noteDemoNotes.value = noteDemoNotes.value.map((note) => (note.id === id ? { ...note, body: value } : note));
  return Promise.resolve(true);
});
export const titleAutosave = createDebouncedAutosave((value: string) => {
  inspectorTitle.value = value.trim();
  return Promise.resolve(true);
});
export const readerTitleAutosave = createDebouncedAutosave((value: string) => {
  readerTitle.value = value.trim();
  return Promise.resolve(true);
});
export const tagsAutosave = createDebouncedAutosave((value: string[]) => {
  inspectorTags.value = value;
  return Promise.resolve(true);
});
