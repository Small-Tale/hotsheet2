import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import type { CodeReview } from '../api';
import { codeReviewTarget, TicketCodeReview } from './ticket-code-review';

const review: CodeReview = {
  difftool: 'glassbox',
  truncated: false,
  ranges: [
    { from: 'aaa1111', to: 'bbb2222', count: 2 },
    { from: 'ccc3333', to: 'ddd4444', count: 2 },
  ],
  commits: [
    {
      sha: 'ddd4444',
      short_sha: 'ddd4444',
      subject: 'HS2-TEST: finish later review run',
      body: 'First detail line\n\nSecond **Markdown** line\nThird hidden line',
      committed_at: '2026-09-02T10:00:00Z',
    },
    {
      sha: 'ccc3333',
      short_sha: 'ccc3333',
      subject: 'HS2-TEST: start later review run',
      committed_at: '2026-09-02T09:00:00Z',
    },
    {
      sha: 'bbb2222',
      short_sha: 'bbb2222',
      subject: 'HS2-TEST: finish review UI',
      committed_at: '2026-09-02T08:00:00Z',
    },
    {
      sha: 'aaa1111',
      short_sha: 'aaa1111',
      subject: 'HS2-TEST: add server route',
      committed_at: '2026-09-01T08:00:00Z',
    },
  ],
};

describe('TicketCodeReview', () => {
  it('HS2-2Z11A5 shows qualified follow-up links and missing targets without commits', () => {
    const markup = String(
      TicketCodeReview({
        review: {
          commits: [],
          ranges: [],
          truncated: false,
          follow_ups: [
            {
              target: { project_id: 'project', connection_id: 'source', native_id: 'one' },
              state: 'resolved',
              slug: 'HS2-ONE',
              title: 'Next step',
            },
            { target: { project_id: 'project', connection_id: 'source', native_id: 'two' }, state: 'missing' },
            { target: { project_id: 'project', connection_id: 'source', native_id: 'three' }, state: 'deleted' },
          ],
        },
      }),
    );
    expect(markup).toContain('Follow-ups (3)');
    expect(markup).toContain('data-action="open-follow-up-target"');
    expect(markup).toContain('data-target-id="@project/source:one"');
    expect(markup).toContain('HS2-ONE · Next step');
    expect(markup).toContain('@project/source:two · Target missing');
    expect(markup).toContain('@project/source:three · Target deleted');
  });
  it('uses canonical review-region spacing while keeping row and action geometry explicit', () => {
    const markup = String(TicketCodeReview({ review }));
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-code-review.css'), 'utf8');
    expect(css).not.toContain('--wa-space-');
    // The inspector tab names the panel; only the embedded review keeps a heading (HS2-E9F1T3).
    expect(markup).not.toContain('ticket-code-review__header');
    expect(markup).not.toContain('role="heading" aria-level="2"');
    const embeddedMarkup = String(TicketCodeReview({ review, embedded: true, title: 'Commits' }));
    expect(embeddedMarkup).toContain('class="kui-toolbar-text" data-component="toolbar-text" data-size="xlarge"');
    expect(embeddedMarkup).toContain('role="heading" aria-level="2"');
    expect(embeddedMarkup).toContain('<div class="ticket-code-review__header"><header class="kui-toolbar"');
    // The diff tool is a footnote, not a trailing heading label (HS2-SHV8J7).
    expect(markup).not.toContain('class="kui-toolbar-text" data-component="toolbar-text" data-size="small"');
    expect(markup).toMatch(
      /<footer class="ticket-code-review__footnotes"><p[^>]*data-tone="quiet"[^>]*>Code reviews open in glassbox\.<\/p><\/footer>/,
    );
    expect(css).toMatchSource(
      /__footnotes \{ display: grid; margin-top: var\(--kui-space-l\); padding-inline: calc\(var\(--kui-space-m\) \+ 1px\)/,
    );
    expect(markup).toMatch(/class="kui-list ticket-code-review__section"[^>]*data-component="list"/);
    expect(markup).toContain('data-component="text" data-tone="default" data-size="compact"');
    expect(markup).toMatch(/class="ticket-code-review__commit-metadata"><div[^>]*data-component="row"/);
    expect(css).not.toMatch(/ticket-code-review--panel \.ticket-code-review__section \{/);
    // The embedded review owns its heading spacing; hosts such as the repository popover never reach in
    // (HS2-4APEJP).
    expect(markup).not.toContain('data-embedded');
    expect(String(TicketCodeReview({ review, embedded: true }))).toContain('data-embedded="true"');
    expect(css).toMatch(
      /\.ticket-code-review\[data-embedded='true'\] \.ticket-code-review__header \{[^}]*margin-bottom: var\(--kui-space-m\)/,
    );
    expect(css).toMatchSource(
      /__evidence \{[^}]*margin-bottom: var\(--kui-space-m\); padding: var\(--kui-space-xs\) var\(--kui-space-m\)/,
    );
    expect(css).toMatch(/__evidence-grid span \{[^}]*gap: var\(--kui-space-2xs\)/);
    expect(css).toMatchSource(
      /__compare-banner \{[^}]*margin-bottom: var\(--kui-space-m\); padding: var\(--kui-space-xs\)[^}]*gap: var\(--kui-space-xs\)/,
    );
    expect(css).toMatch(
      /__range \{[^}]*padding: var\(--kui-space-xs\) var\(--kui-space-m\)[^}]*gap: var\(--kui-space-xs\)/,
    );
    expect(css).toMatch(
      /__commit \{[^}]*min-height: remify\(68px\)[^}]*padding: var\(--kui-space-xs\) calc\(var\(--kui-space-m\) \+ 1px\)[^}]*gap: var\(--kui-space-xs\)/,
    );
    expect(css).toMatch(/__commit-summary \{[^}]*gap: var\(--kui-space-2xs\)/);
    expect(css).toMatchSource(/__notice \{ margin: 0; padding: var\(--kui-space-xs\)/);
    // Kerf EmptyState and Chip own the empty state and ref tags (HS2-GQXT52).
    expect(css).not.toMatch(/__(empty|ref) \{/u);
    expect(css).not.toContain('!important');
    expect(css).toMatchSource(/__refs \{[^}]*gap: var\(--kui-space-2xs\); margin-top: var\(--kui-space-2xs\)/);
  });

  it('leaves Change Evidence unfilled at rest and rules it off from the first commit (HS2-3F2DZN)', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-code-review.css'), 'utf8');
    expect(css).toMatch(/\.ticket-code-review__evidence \{[^}]*background: transparent;/);
    expect(css).toMatch(
      /\.ticket-code-review__evidence:hover \{[^}]*background: var\(--wa-color-neutral-fill-normal\)/,
    );
    expect(css).toMatch(
      /\.ticket-code-review__commits \{[^}]*border-top: 1px solid var\(--wa-color-neutral-border-quiet\)/,
    );
  });

  it('ends the commit list with whitespace instead of a closing rule (HS2-X8XK9D)', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-code-review.css'), 'utf8');
    expect(css).toMatchSource(/\.ticket-code-review__commits > :last-child \{ border-bottom: 0; \}/);
    expect(css).toMatch(/__footnotes \{[^}]*margin-top: var\(--kui-space-l\)/);
  });

  it('resets the native list-item indent so commit rows align with their list', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-code-review.css'), 'utf8');
    expect(css).toMatch(/\.ticket-code-review__commit \{[^}]*margin-inline-start: 0;/);
  });

  it('lists commit messages and exposes only server-provided commit and range targets', () => {
    const markup = String(TicketCodeReview({ review }));
    expect(markup).toContain('Code reviews open in glassbox');
    expect(markup).not.toContain('Opens in');
    expect(markup).toContain('HS2-TEST: finish review UI');
    expect(markup).toContain('data-review-mode="range" data-review-from="aaa1111" data-review-to="bbb2222"');
    expect(markup).toContain('data-review-mode="range" data-review-from="ccc3333" data-review-to="ddd4444"');
    expect(markup.match(/class="ticket-code-review__range"/g)).toHaveLength(2);
    expect(markup).toContain('Open 2-commit bundle<small>aaa1111 → bbb2222</small>');
    expect(markup).toContain('Open 2-commit bundle<small>ccc3333 → ddd4444</small>');
    expect(markup).toContain('data-review-mode="commit" data-review-commit="bbb2222"');
    expect(markup).toContain('data-lucide="external-link"');
    const laterRange = markup.indexOf('data-review-from="ccc3333"');
    const latestCommit = markup.indexOf('class="ticket-code-review__commit" data-commit-sha="ddd4444"');
    const earlierRange = markup.indexOf('data-review-from="aaa1111"');
    const earlierCommit = markup.indexOf('class="ticket-code-review__commit" data-commit-sha="bbb2222"');
    expect(laterRange).toBeLessThan(latestCommit);
    expect(earlierRange).toBeGreaterThan(markup.indexOf('data-commit-sha="ccc3333"'));
    expect(earlierRange).toBeLessThan(earlierCommit);
  });

  it('shows the commit window as an unfilled footnote above the diff tool footnote (HS2-1VWZ9S)', () => {
    const markup = String(TicketCodeReview({ review: { ...review, truncated: true } }));
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-code-review.css'), 'utf8');
    expect(markup).toMatch(
      /<footer class="ticket-code-review__footnotes"><p[^>]*data-tone="quiet"[^>]*>Showing matches from the newest 2,000 commits\.<\/p><p[^>]*>Code reviews open in glassbox\.<\/p><\/footer>/,
    );
    expect(markup).not.toMatch(/ticket-code-review__notice[^>]*>(?:(?!<\/div>).)*newest 2,000/su);
    expect(css).not.toMatch(/__footnotes \{[^}]*background/);
    // Without commits or without truncation there is no commit-window footnote.
    expect(String(TicketCodeReview({ review }))).not.toContain('newest 2,000');
    expect(String(TicketCodeReview({ review: { ...review, truncated: true, commits: [], ranges: [] } }))).not.toContain(
      'newest 2,000',
    );
  });

  it('decorates commits with their git refs classified by kind (HS2-SFJ5TE)', () => {
    const decorated: CodeReview = {
      ...review,
      commits: [
        {
          sha: 'ddd4444',
          short_sha: 'ddd4444',
          subject: 'Tip commit',
          committed_at: '2026-09-02T10:00:00Z',
          refs: [
            { label: 'HEAD → main', kind: 'head' },
            { label: 'origin/main', kind: 'remote' },
            { label: 'v1.2.0', kind: 'tag' },
            { label: 'release', kind: 'branch' },
          ],
        },
        { sha: 'aaa1111', short_sha: 'aaa1111', subject: 'Older commit', committed_at: '2026-09-01T08:00:00Z' },
      ],
    };
    const markup = String(TicketCodeReview({ review: decorated }));
    expect(markup).toContain('data-ref-kind="head"');
    expect(markup).toContain('data-ref-kind="remote"');
    expect(markup).toContain('data-ref-kind="tag"');
    expect(markup).toContain('data-ref-kind="branch"');
    expect(markup).toContain('HEAD → main');
    expect(markup).toContain('v1.2.0');
    expect(markup).toContain('data-lucide="tag"');
    expect(markup).toContain('data-lucide="git-branch"');
    expect(markup).toContain('title="Current HEAD: HEAD → main"');
    expect(markup).toContain('title="Tag: v1.2.0"');
    // Only the tip commit is decorated; the older commit has no ref chips.
    expect(markup.match(/ticket-code-review__ref"/g)).toHaveLength(4);
    expect(markup.match(/ticket-code-review__refs"/g)).toHaveLength(1);
  });

  it('marks unmerged worktree commits with a chip and a summary notice (HS2-0WZRK5)', () => {
    const pending: CodeReview = {
      ...review,
      commits: [
        {
          sha: 'fff6666',
          short_sha: 'fff6666',
          subject: 'Worktree tip',
          committed_at: '2026-09-03T10:00:00Z',
          unmerged: { branch: 'feature', worktree: '/repo/.claude/worktrees/agent-1' },
        },
        {
          sha: 'eee5555',
          short_sha: 'eee5555',
          subject: 'Detached experiment',
          committed_at: '2026-09-03T09:00:00Z',
          unmerged: { worktree: 'C:\\work\\detached' },
        },
        {
          sha: 'ddd4444',
          short_sha: 'ddd4444',
          subject: 'Branch only',
          committed_at: '2026-09-03T08:00:00Z',
          unmerged: { branch: 'topic' },
        },
        { sha: 'aaa1111', short_sha: 'aaa1111', subject: 'Merged', committed_at: '2026-09-01T08:00:00Z' },
      ],
    };
    const markup = String(TicketCodeReview({ review: pending }));
    expect(markup.match(/ticket-code-review__unmerged"/g)).toHaveLength(3);
    expect(markup).toContain('Unmerged · feature in agent-1');
    expect(markup).toContain('Unmerged · detached');
    expect(markup).toContain('Unmerged · topic');
    expect(markup).toContain('data-lucide="folder-git-2"');
    expect(markup).toContain(
      'title="Not merged into this checkout yet · branch feature · worktree /repo/.claude/worktrees/agent-1"',
    );
    expect(markup).toContain('3 commits are not merged into this checkout yet (feature in agent-1, detached, topic).');
    // None of these commits carries git refs, so no ref row renders.
    expect(markup).not.toContain('ticket-code-review__refs"');

    const one = String(TicketCodeReview({ review: { ...pending, commits: pending.commits.slice(2) } }));
    expect(one).toContain('1 commit is not merged into this checkout yet (topic).');
    const none = String(TicketCodeReview({ review: { ...pending, commits: pending.commits.slice(3) } }));
    expect(none).not.toContain('data-unmerged-notice');
  });

  it('HS2-39D5YS explains a stale checkout separately from genuinely pending work', () => {
    const markup = String(
      TicketCodeReview({
        review: {
          ...review,
          checkout_behind_main: true,
          commits: [{ ...review.commits[0], unmerged: { branch: 'later-work' } }, review.commits[1]],
        },
      }),
    );
    expect(markup).toContain('data-checkout-behind-main="true"');
    expect(markup).toContain('Recorded ticket work is on remote main. This checkout is behind');
    expect(markup).toContain('1 commit is not merged into this checkout yet (later-work).');
  });

  it('summarizes classified changes and calls out modified existing tests', () => {
    const markup = String(
      TicketCodeReview({
        review: {
          ...review,
          summary: { files: { total: 8, docs: 2, tests: 3, source: 2, other: 1 }, tests_added: 2, tests_modified: 1 },
        },
      }),
    );
    expect(markup).toContain('aria-label="Open Change Evidence"');
    expect(markup).toContain('data-action="open-change-evidence"');
    expect(markup).toContain('<strong>2</strong> docs');
    expect(markup).toContain('<strong>3</strong> tests');
    expect(markup).toContain('2 new test files · 1 existing test file modified');
    expect(markup).toContain('data-tests-modified="true"');
    expect(markup).toContain('<div class="ticket-code-review__evidence-grid">');
    // The evidence row is an app-owned flex row, not a restyled Kerf Grid (HS2-57MAAH).
    expect(markup).not.toContain('data-columns="2"');
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-code-review.css'), 'utf8');
    expect(css).toMatch(/__evidence \{[^}]*height: auto;[^}]*overflow: hidden;[^}]*box-sizing: border-box;/);
    expect(css).toContainSource('.ticket-code-review__evidence-grid { grid-template-columns: 1fr; }');
  });

  it('keeps history readable but disables launching without a configured tool', () => {
    const markup = String(TicketCodeReview({ review: { ...review, difftool: undefined } }));
    expect(markup).toContain('No Git diff tool is configured');
    expect(markup).toContain('HS2-TEST: add server route');
    expect(markup.match(/ disabled/g)).toHaveLength(6);
  });

  it('renders loading and empty states', () => {
    expect(String(TicketCodeReview({ loading: true }))).toContain('Finding ticket commits');
    expect(
      String(TicketCodeReview({ review: { commits: [], ranges: [], difftool: 'meld', truncated: false } })),
    ).toContain('No commits referencing this ticket');
  });

  it('supports the repository dialog while preserving the shared commit presentation', () => {
    const markup = String(
      TicketCodeReview({
        review,
        embedded: true,
        title: 'Commits',
        action: 'open-repository-review',
        comparison: { active: true, side: 'b', a: 'aaa1111', b: 'ddd4444' },
        expandedCommits: ['ddd4444'],
      }),
    );
    expect(markup).not.toContain('ticket-inspector-panel');
    expect(markup).toContain('role="heading" aria-level="2"');
    expect(markup).toContain('<span class="kui-toolbar-text__text">Commits</span>');
    expect(markup).toContain('data-action="open-repository-review"');
    expect(markup).toContain('data-lucide="git-compare"');
    expect(markup).toContain('Select the <strong>B</strong> side');
    expect(markup).toContain('<div class="ticket-code-review__compare-toolbar"><header class="kui-toolbar"');
    expect(markup).toContain('data-review-mode="compare" data-review-from="aaa1111" data-review-to="ddd4444"');
    expect(markup).toContain('class="ticket-code-review__compare-label">A</b>');
    expect(markup).toContain('class="ticket-code-review__compare-label">B</b>');
    expect(markup).toContain('Third hidden line');
    expect(markup).not.toContain('cancel-repository-comparison');
    expect(markup).not.toContain('start-repository-comparison');
  });

  it('limits collapsed commit detail to two Markdown lines', () => {
    const markup = String(TicketCodeReview({ review }));
    expect(markup).toContain('First detail line');
    expect(markup).toContain('Second <strong>Markdown</strong> line');
    expect(markup).not.toContain('Third hidden line');
  });

  it('decodes only complete action datasets', () => {
    expect(codeReviewTarget({ reviewMode: 'commit', reviewCommit: 'abc' })).toEqual({ mode: 'commit', commit: 'abc' });
    expect(codeReviewTarget({ reviewMode: 'range', reviewFrom: 'abc', reviewTo: 'def' })).toEqual({
      mode: 'range',
      from: 'abc',
      to: 'def',
    });
    expect(codeReviewTarget({ reviewMode: 'compare', reviewFrom: 'abc', reviewTo: 'def' })).toEqual({
      mode: 'compare',
      from: 'abc',
      to: 'def',
    });
    expect(codeReviewTarget({ reviewMode: 'compare', reviewFrom: 'abc', reviewTo: 'abc' })).toBeUndefined();
    expect(codeReviewTarget({ reviewMode: 'ticket_file', reviewPath: 'src/main.ts' })).toEqual({
      mode: 'ticket_file',
      paths: ['src/main.ts'],
    });
    expect(codeReviewTarget({ reviewMode: 'worktree_file', reviewPath: 'src/main.ts', reviewArea: 'staged' })).toEqual({
      mode: 'worktree_file',
      paths: ['src/main.ts'],
      area: 'staged',
    });
    expect(codeReviewTarget({ reviewMode: 'range', reviewFrom: 'abc' })).toBeUndefined();
  });
});
