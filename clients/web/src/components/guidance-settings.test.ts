import { describe, expect, it } from 'vitest';

import { createGuidanceSearchModel } from '../guidance-search-model';
import { GuidanceSettings } from './guidance-settings';

const emptyGuidanceSearch = createGuidanceSearchModel(() => undefined).state.value;

describe('GuidanceSettings rule summaries', () => {
  it('HS2-019VVA groups long catalogs, preserves selections, and shows protected rule previews', () => {
    const rules = Array.from({ length: 36 }, (_, index) => ({
      name: `rule-${index}`,
      description: `Guidance rule ${index}`,
      version: 2,
      installed_version: index === 0 ? 1 : null,
      owner: index < 18 ? 'Hot Sheet' : 'Kerf',
      tags: index === 0 ? ['rust', 'testing'] : ['testing'],
      selected: index === 0,
    }));
    const base = {
      loading: false,
      message: '',
      search: emptyGuidanceSearch,
      repository: '',
      sourceRef: '',
      sourceId: 'catalog',
      ownerFilter: '',
      tagFilter: '',
      overview: {
        owner: 'builtin' as const,
        explicit_selection: true,
        default_repository: '',
        sources: [
          {
            id: 'catalog',
            repository: 'https://example.test/rules',
            requested_ref: 'main',
            commit: 'a'.repeat(40),
            rules,
            skills: [],
          },
        ],
      },
    };
    const grouped = String(GuidanceSettings(base));
    expect(grouped).toContain('Hot Sheet · 18 rules');
    expect(grouped).toContain('Kerf · 18 rules');
    expect(grouped).toContain('1 selected');
    expect(grouped).toContain('0 selected');
    expect(grouped).toContain('rust');
    expect(grouped).toContain('data-guidance-rule="rule-0"');
    expect(grouped).toContain('data-action="preview-guidance-rule"');
    const filtered = String(GuidanceSettings({ ...base, ownerFilter: 'Hot Sheet', tagFilter: 'rust' }));
    expect(filtered).toContain('Hot Sheet · 1 rule');
    expect(filtered).not.toContain('Kerf · 18 rules');
    const preview = String(
      GuidanceSettings({
        ...base,
        rulePreview: {
          source: 'catalog',
          rule: 'rule-0',
          loading: false,
          error: '',
          result: {
            owner: 'builtin' as const,
            current: 'Local edit',
            pinned: 'Pinned v2',
            after: 'Local edit',
            outcome: 'protected' as const,
          },
        },
      }),
    );
    expect(preview).toContain('Local edits or a newer installed version are protected');
    expect(preview).toContain('Pinned Rule Offered');
    expect(preview).toContain('Pinned v2');
    expect(preview).toContain('Close Preview');
  });
  it('HS2-T0GHX5 starts setup with Instruction Writer without repeating the navigation title', () => {
    const markup = String(
      GuidanceSettings({
        loading: false,
        message: '',
        search: emptyGuidanceSearch,
        repository: '',
        sourceRef: '',
        sourceId: '',
        ownerFilter: '',
        tagFilter: '',
        overview: { owner: 'rulesync', explicit_selection: false, default_repository: '', sources: [] },
      }),
    );
    expect(markup).not.toContain('>Project guidance</');
    expect(markup).not.toContain('Choose a pinned rule source and the guidance this project receives.');
    expect(markup.indexOf('Instruction Writer')).toBeLessThan(markup.indexOf('Guidance Recommendations'));
    expect(markup.indexOf('Rulesync regenerates')).toBeLessThan(markup.indexOf('Guidance Recommendations'));
  });

  it('HS2-C9FC2K shows the pending Rulesync review action until a source is linked', () => {
    const props = {
      loading: false,
      message: '',
      search: emptyGuidanceSearch,
      repository: '',
      sourceRef: '',
      sourceId: '',
      ownerFilter: '',
      tagFilter: '',
    };
    const overview = { owner: 'rulesync' as const, explicit_selection: false, default_repository: '', sources: [] };
    const pending = String(
      GuidanceSettings({ ...props, overview: { ...overview, pending_consolidation_review: true } }),
    );
    expect(pending).toContain('Rulesync Guidance Review Pending');
    expect(pending).toContain('Ticket sources');
    expect(pending).toContain('check-rulesync-updates');
    expect(
      String(GuidanceSettings({ ...props, overview: { ...overview, pending_consolidation_review: false } })),
    ).not.toContain('Rulesync Guidance Review Pending');
  });
  it('shows exact interview preview, consent, advisory answer, and unknown status (HS2-YGV9RN)', () => {
    const base = {
      loading: false,
      message: '',
      search: emptyGuidanceSearch,
      repository: '',
      sourceRef: '',
      sourceId: '',
      ownerFilter: '',
      tagFilter: '',
    };
    const interview = {
      state: 'not_started' as const,
      preview: {
        fingerprint: 'review-1',
        context: {
          signals: ['react', 'vue'],
          candidates: [
            {
              id: 'candidate-1',
              source: 'source',
              kind: 'rule' as const,
              name: 'react-review',
              version: 1,
              pinned_commit: 'a'.repeat(40),
              selected: false,
              evidence: ['package.json: react'],
            },
          ],
        },
        questions: [{ id: 'active-framework', prompt: 'Which framework is active?', choices: ['react', 'vue'] }],
        outbound_prompt: 'Exact outbound JSON',
      },
      answer: null,
    };
    const preview = String(GuidanceSettings({ ...base, interview }));
    expect(preview).toContain('Exact outbound JSON');
    expect(preview).toContain('I Choose to Share This Context');
    expect(preview).not.toContain('Copy Prompt for AI');
    const awaiting = String(GuidanceSettings({ ...base, interview: { ...interview, state: 'awaiting_answer' } }));
    expect(awaiting).toContain('Copy Prompt for AI');
    expect(awaiting).toContain('AI Advisory JSON');
    const answered = String(
      GuidanceSettings({
        ...base,
        interview: {
          ...interview,
          state: 'answered',
          answer: { advice: [{ candidate_id: 'candidate-1', stance: 'uncertain', explanation: 'Ask a follow-up.' }] },
        },
      }),
    );
    expect(answered).toContain('Ask a follow-up.');
    expect(answered).not.toContain('Review Advisory Answer');
    expect(String(GuidanceSettings({ ...base, interviewError: 'Offline.' }))).toContain('Interview Status Unknown');
  });
  it('shows local recommendation changes and individual decision controls (HS2-QBXRFY)', () => {
    const base = {
      loading: false,
      message: '',
      search: emptyGuidanceSearch,
      repository: '',
      sourceRef: '',
      sourceId: '',
      ownerFilter: '',
      tagFilter: '',
    };
    const recommendations = {
      fingerprint: 'scan',
      signals: [{ key: 'rust', evidence: 'Cargo.toml' }],
      candidates: [
        {
          source: 'rules',
          kind: 'rule' as const,
          name: 'rust-lint',
          version: 2,
          description: 'Rust lint and formatting',
          pinned_commit: 'abcdef1234567890',
          evidence: ['Cargo.toml'],
          fingerprint: 'candidate',
          selected: false,
          installed_version: null,
          state: 'new' as const,
        },
      ],
      changes: [{ source: 'rules', kind: 'rule' as const, name: 'rust-lint', change: 'new' as const }],
      source_registration_suggested: false,
      first_accept_replaces_bundled_default: true,
    };
    const markup = String(GuidanceSettings({ ...base, recommendations }));
    expect(markup).toContain('New Guidance Recommendations Are Available');
    expect(markup).toContain('Rust lint and formatting');
    expect(markup).toContain('Evidence: Cargo.toml');
    expect(markup).toContain('data-recommendation-fingerprint="candidate"');
    expect(markup).toContain('Accept Selection');
    expect(markup).toContain('Dismiss');
    expect(markup).toContain('first accepted selection replaces the bundled default');
    const dismissed = String(
      GuidanceSettings({
        ...base,
        recommendations: {
          ...recommendations,
          changes: [],
          candidates: [{ ...recommendations.candidates[0], state: 'dismissed' as const }],
        },
      }),
    );
    expect(dismissed).not.toContain('New Guidance Recommendations Are Available');
    expect(dismissed).toContain('dismissed');
    const offline = String(GuidanceSettings({ ...base, recommendationsError: 'Local scan unavailable.' }));
    expect(offline).toContain('Recommendation Status Unknown');
    expect(offline).not.toContain('No guidance suggestions match');
  });
  it('separates changed upstream revisions, pending pinned installation, and failed checks', () => {
    const base = {
      loading: false,
      message: '',
      search: emptyGuidanceSearch,
      repository: '',
      sourceRef: 'main',
      sourceId: 'fixture',
      ownerFilter: '',
      tagFilter: '',
      overview: {
        owner: 'builtin' as const,
        explicit_selection: true,
        default_repository: '',
        sources: [
          {
            id: 'fixture',
            repository: 'https://example.test/source.git',
            requested_ref: 'main',
            commit: '1111111111111111111111111111111111111111',
            rules: [
              {
                name: 'example',
                description: 'Example rule',
                version: 2,
                installed_version: 1,
                owner: null,
                tags: [],
                selected: true,
              },
            ],
            skills: [],
          },
        ],
      },
    };
    const update = {
      id: 'fixture',
      requested_ref: 'main',
      pinned_commit: base.overview.sources[0].commit,
      observed_commit: '2222222222222222222222222222222222222222',
      state: 'revision_changed' as const,
    };
    const changed = String(
      GuidanceSettings({
        ...base,
        updates: [update],
        review: {
          update,
          changes: [
            { name: 'example', kind: 'rule', change: 'updated', selected: true, before_version: 2, after_version: 3 },
          ],
        },
      }),
    );
    expect(changed).toContain('fixture Has A Different Source Revision');
    expect(changed).toContain('Review fixture');
    expect(changed).toContain('rule example: updated v2 → v3');
    expect(changed).toContain('1 selected rule(s) still need Apply Selected Guidance');
    const failed = String(
      GuidanceSettings({
        ...base,
        updates: [{ ...update, state: 'check_failed' }],
        updatesError: 'Remote check failed.',
      }),
    );
    expect(failed).toContain('Source Check Failed');
    expect(failed).toContain('Previous results may be stale');
    expect(failed).not.toContain('Refresh Reviewed Pin');
    const current = String(GuidanceSettings({ ...base, updates: [{ ...update, state: 'current' }] }));
    expect(current).toContain('source is current');
    expect(current).toContain('selected rule(s) still need Apply Selected Guidance');
    const removed = String(
      GuidanceSettings({
        ...base,
        review: {
          update,
          changes: [
            {
              name: 'example',
              kind: 'rule',
              change: 'removed',
              selected: true,
              before_version: 2,
              after_version: null,
            },
          ],
        },
      }),
    );
    expect(removed).toContain('Deselect and apply those entries at the old pin');
    expect(removed).toMatch(
      /data-action="refresh-reviewed-guidance"[^>]*disabled|disabled[^>]*data-action="refresh-reviewed-guidance"/,
    );
  });
  it('shows standalone Rulesync lock freshness and keeps the executable version separate', () => {
    const base = {
      loading: false,
      message: '',
      search: emptyGuidanceSearch,
      repository: '',
      sourceRef: '',
      sourceId: '',
      ownerFilter: '',
      tagFilter: '',
    };
    for (const state of ['current', 'outdated', 'unlocked', 'unknown'] as const) {
      const markup = String(
        GuidanceSettings({
          ...base,
          rulesyncUpdateReport: {
            state,
            executable_version: '38.0.0',
            sources: [{ source: 'fixture', state, requested_ref: 'main', locked_ref: 'first', latest_ref: 'second' }],
          },
        }),
      );
      expect(markup).toContain('data-rulesync-update-status');
      expect(markup).toContain('Rulesync 38.0.0');
      expect(markup).toContain(`fixture: ${state}`);
      expect(markup).toContain('locked first');
    }
    expect(String(GuidanceSettings(base))).not.toContain('data-rulesync-update-status');
  });
  it('shows only blocked newer bundled sections as a review notice', () => {
    const props = {
      loading: false,
      message: '',
      search: emptyGuidanceSearch,
      repository: '',
      sourceRef: '',
      sourceId: '',
      ownerFilter: '',
      tagFilter: '',
    };
    expect(String(GuidanceSettings(props))).not.toContain('Bundled Guidance Needs Review');
    const markup = String(
      GuidanceSettings({
        ...props,
        bundledGuidanceBlocked: [
          { path: 'AGENTS.md', section: 'ticket-driven-work', installedVersion: 1, bundledVersion: 6 },
        ],
      }),
    );
    expect(markup).toContain('Bundled Guidance Needs Review');
    expect(markup).toContain('AGENTS.md: ticket-driven-work (local v1, bundled v6)');
    expect(markup).toContain('Hot Sheet preserved the local text');
  });
  it('leads with the human description and keeps the slug with the available version', () => {
    const markup = String(
      GuidanceSettings({
        loading: false,
        message: '',
        search: emptyGuidanceSearch,
        repository: '',
        sourceRef: '',
        sourceId: '',
        ownerFilter: '',
        tagFilter: '',
        overview: {
          owner: 'builtin',
          explicit_selection: true,
          default_repository: '',
          sources: [
            {
              id: 'rules',
              repository: 'https://example.test/rules',
              requested_ref: 'v1',
              commit: '1234567890abcdef',
              rules: [
                {
                  name: 'rust-lint-and-format',
                  description: 'Rust lint and formatting defaults',
                  version: 4,
                  installed_version: 3,
                  owner: 'rust',
                  tags: ['rust'],
                  selected: true,
                },
                {
                  name: 'hotsheet-visual-qa',
                  description: 'Visual review of rendered changes',
                  version: 2,
                  installed_version: null,
                  owner: 'hotsheet',
                  tags: ['testing'],
                  selected: false,
                },
              ],
              skills: [],
            },
          ],
        },
      }),
    );
    expect(markup).toMatch(/class="kui-checkbox__label"[^>]*>Rust lint and formatting defaults/);
    expect(markup).toContain('rust-lint-and-format · available v4 · v3 installed');
    expect(markup).toContain('hotsheet-visual-qa · available v2 · not installed');
  });
});
