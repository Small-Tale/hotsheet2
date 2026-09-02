import { beforeEach, describe, expect, it } from 'vitest';

import {
  PermissionRequestDemo,
  PermissionRequestSettings,
  permissionRequestSettings,
  resetPermissionRequestDemo,
} from './permission-components-demo';

describe('PermissionRequestCard demo settings', () => {
  beforeEach(() => {
    resetPermissionRequestDemo();
  });

  it('exposes the important presentation, lifecycle, request, and capability variants', () => {
    const markup = String(PermissionRequestSettings());
    for (const name of ['presentation', 'variant', 'request', 'automation', 'always-supported', 'explanation']) {
      expect(markup).toContain(`name="${name}"`);
    }
    for (const variant of ['pending', 'resolving', 'failed', 'disconnected', 'allowed', 'denied', 'external']) {
      expect(markup).toContain(`value="${variant}"`);
    }
  });

  it('projects pending, failure, and history variants through the real component', () => {
    expect(String(PermissionRequestDemo())).toContain('permission-request-popup');
    permissionRequestSettings.variant.value = 'failed';
    expect(String(PermissionRequestDemo())).toContain('data-state="failed"');
    expect(String(PermissionRequestDemo())).toContain('could not be delivered');
    permissionRequestSettings.variant.value = 'disconnected';
    expect(String(PermissionRequestDemo())).toContain('data-state="disconnected"');
    permissionRequestSettings.variant.value = 'allowed';
    const allowed = String(PermissionRequestDemo());
    expect(allowed).toContain('data-state="allow"');
    expect(allowed).toContain('allowed this kind of request');
    expect(allowed).not.toContain('resolve-permission');
    permissionRequestSettings.variant.value = 'external';
    expect(String(PermissionRequestDemo())).toContain('Decision made outside Hot Sheet');
  });

  it('projects request details, countdown direction, capabilities, and list presentation', () => {
    permissionRequestSettings.presentation.value = 'list';
    permissionRequestSettings.request.value = 'command';
    permissionRequestSettings.automation.value = 'deny';
    let markup = String(PermissionRequestDemo());
    expect(markup).toContain('permission-request-card--list');
    expect(markup).toContain('Wants permission to run a command');
    expect(markup).toContain('Automatically denied in');

    permissionRequestSettings.request.value = 'tool-without-details';
    permissionRequestSettings.alwaysSupported.value = false;
    permissionRequestSettings.explanation.value = false;
    markup = String(PermissionRequestDemo());
    expect(markup).not.toContain('permission-request-card__details');
    expect(markup).not.toContain('Always Allow');
    expect(markup).not.toContain('permission-request-card__explanation');
  });

  it('resets every setting to the canonical review state', () => {
    permissionRequestSettings.presentation.value = 'list';
    permissionRequestSettings.variant.value = 'denied';
    permissionRequestSettings.request.value = 'read';
    permissionRequestSettings.automation.value = 'none';
    permissionRequestSettings.alwaysSupported.value = false;
    permissionRequestSettings.explanation.value = false;
    resetPermissionRequestDemo();
    expect({
      presentation: permissionRequestSettings.presentation.value,
      variant: permissionRequestSettings.variant.value,
      request: permissionRequestSettings.request.value,
      automation: permissionRequestSettings.automation.value,
      alwaysSupported: permissionRequestSettings.alwaysSupported.value,
      explanation: permissionRequestSettings.explanation.value,
    }).toEqual({
      presentation: 'popup', variant: 'pending', request: 'edit', automation: 'allow',
      alwaysSupported: true, explanation: true,
    });
  });
});
