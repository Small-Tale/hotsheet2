import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { sourceTokens } from './source-format-matchers';

describe('main interaction wiring (HS2-3KQ365)', () => {
  const source = readFileSync(new URL('./main.tsx', import.meta.url), 'utf8'),
    tokens = sourceTokens(source);
  const groups = [
    'wireProjectLifecycleInteractions',
    'wireRepositoryInteractions',
    'wireNavigationAndTabInteractions',
    'wireTerminalInteractions',
    'wireTicketSelectionInteractions',
    'wireViewAndSavedViewInteractions',
    'wireCommandAndAiInteractions',
    'wireNotificationAndLinkInteractions',
    'wireSearchAndComposerInteractions',
    'wireAttachmentAndGalleryInteractions',
    'wireInspectorAndEditorInteractions',
    'wireShellAndGlobalInteractions',
  ];

  it('declares and invokes the feature groups in registration order', () => {
    let cursor = -1;

    for (const group of groups) {
      const declaration = tokens.indexOf(`function${group}(){`);
      const invocation = tokens.indexOf(`${group}();`, declaration);

      expect(declaration, `${group} declaration`).toBeGreaterThan(cursor);
      expect(invocation, `${group} invocation`).toBeGreaterThan(declaration);
      cursor = invocation;
    }
  });

  it('keeps every delegated handler inside the hierarchy on the single body root', () => {
    const wiringStart = tokens.indexOf(`function${groups[0]}(){`);
    const wiringEnd = tokens.indexOf(`${groups.at(-1)}();`, wiringStart) + `${groups.at(-1)}();`.length;
    const registrations = [...tokens.matchAll(/delegate(?:Capture)?\(([^,]+),/g)];

    expect(registrations).toHaveLength(404);
    for (const registration of registrations) {
      expect(registration.index).toBeGreaterThan(wiringStart);
      expect(registration.index).toBeLessThan(wiringEnd);
      expect(registration[1]).toBe('document.body');
    }
  });

  it('routes token-search edits through the Kerf beta 22 wiring callback', () => {
    expect(source).toContainSource("onEdit:({id,editor,event})=>{if(id!=='workspace-search')return;");
    expect(source).not.toContain("delegate(document.body,'input','[data-token-search-editor=\"workspace-search\"]'");
  });

  it('routes both project and drawer tab strips through the shared Kerf wiring', () => {
    expect(source).toContainSource('if(barId===PROJECT_TAB_BAR_ID)');
    expect(source).toContainSource('if(barId!==TERMINAL_DRAWER_TAB_BAR_ID)return;');
    expect(source).not.toContain('DRAWER_APP_TAB_SELECTOR');
    expect(source).not.toContain('application/x-hotsheet-app-tab');
  });
});
