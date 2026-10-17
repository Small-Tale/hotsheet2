# Source CSS ownership audit

Audit for `HS2-JMYRT3`, against installed Kerf UI `5.0.0-beta.76` and its AI guidance. The baseline was captured before the current migration: **103 source CSS files, 11,220 lines, approximately 6,747 declarations**, including **97 component CSS files, 9,665 lines, approximately 5,793 declarations**. Declaration counts use a lexical scan and are approximate. The 35 JSX `style=` occurrences include semantic component props such as `style="crossfade"`; they are not all inline CSS.

Every stylesheet below was inspected for selectors and declaration purposes. A generic replacement finding does not mean every declaration in that file should disappear. Ownership follows the component that creates and controls the markup. Raw Markdown, terminal canvas, charts, media annotations, semantic list markup, and application interaction geometry can justify CSS. Thin compositions should use public components and props without a stylesheet.

## Current migration versus baseline

This migration scope covers document baseline, application/inspector empty states, connecting feedback, notification queues/navigation, settings navigation, and terminal operations sidebar. The seven original stylesheet removal candidates are `app-empty-state.css`, `ticket-inspector-placeholder.css`, `connection-state-banner.css`, `notification-navigation.css`, `settings-navigation.css`, `terminal-operations-sidebar.css`, and `notification-center.css`. This audit lists their before-change state; final implementation counts and verification must be recorded separately. `style.css` and notification inspector empty placement also participate in the composition review. The migration also removes `notification-inspector.css`, for eight stylesheet removals. The shell uses block flow so the Workbench fills its host independently of content intrinsic width. Compact chart summaries and headings share the standard control gutter. Remaining generic work is explicitly tracked below; this first migration does not claim it is all removed.

Existing follow-up boundaries:

- **Dialogs:** `HS2-XB41PF`.
- **Feedback, attribution, toasts:** `HS2-KEHG7H`.
- **Command/view/repository navigation:** `HS2-8K7QK7`.

Further independent scopes identified below: settings/forms; notes/editors/conflicts; ticket presentations; shell/project chrome; terminal chrome; attachments/media controls; metadata adapters; development/catalog UI. These labels describe implementation boundaries, not completion claims. Ticket identities: settings/forms `HS2-J7BTAG`; notes/editors `HS2-ZA1EAG`; ticket presentations `HS2-WA2Q12`; shell/project `HS2-TQDZET`; terminal chrome `HS2-1Q6GER`; attachments `HS2-VHY325`; metadata `HS2-PQHN1J`; development/catalog UI `HS2-3AZ08H`.

## Exact API blockers

- **Heading icon tile tone (`KF-VH4B52`):** `ToolbarControlGroupTone` is only `'default' | 'dark'`. It does not express the existing brand/success/danger icon tile fills. `selectedTone` describes selected control chrome, not an arbitrary resting icon tile. Retain the counted finding until the exact API ships; do not call this a stale suppression because a generic `tone` prop exists.
- **Workspace width thresholds (`KF-MXE9YV`):** `ToolbarControlGroupVisibility` is only `'always' | 'compact-only' | 'hide-collapsed-tiny' | 'yield-to-expanded-sibling'`. It does not express the consumer's independent 480/416/224/176px thresholds, rail exceptions, or identity visibility. Retain the counted finding until the exact contract exists.
- **Grouped search anchor (`KF-M8SV15`):** grouped search expansion/anchoring is not supplied merely by existing `overflow` choices (`'visible' | 'scroll' | 'wrap'`). Replace generic date/help body markup independently, preserving the anchoring finding.
- **Gallery filename foreground (`KF-7288MD`):** installed `ToolbarText` has no tone/color prop. A group dark tone is not an equivalent declaration for filename foreground. Preserve the counted finding until exact public configuration exists.

The current doctor suppressions are narrow, counted pending API boundaries. Their existence does not waive unrelated raw markup or external component styling. Never copy removed application CSS onto a Kerf root hook. Use `Row`, `List`, `Grid`, `Text`, `FieldLabel`, `Badge`, `Chip`, `EmptyState`, `StateBanner`, `Toolbar`, `ToolbarControlGroup`, `Pane`, `SplitView`, `DialogSurface`, `PopupMenu`, and supported form primitives when their semantics match. Keep semantic `ul/ol`, tables, and file input markup where required.

`DialogSurface` is exported from `@kerfjs/ui/surface-scaffold` and wraps a direct raw `<wa-dialog>`. There is no Kerf `Dialog` JSX export. Native popover shells require an explicit behavior migration before replacing their shell CSS.

The current result contains **95 source CSS files, 11,092 lines, approximately 6,685 declarations**. The inventory below preserves the original 103-file baseline.

## File manifest

All paths are repository-relative. **Current** means the primary's active scope; other scope names map to the follow-up boundaries above or to independently identified work. **Retain** means no wholesale replacement was established, not a permanent exemption from inspection.

| Stylesheet                                                    | Ownership rationale and remaining generic compositions                                                                                                                                                                                                                          | Scope                                              |
| ------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| `clients/web/src/components/active-claim.css`                 | ETA ring is owned raw Web Awesome; notice frame, text, row spacing and ETA layout map to StateBanner/Text/Row.                                                                                                                                                                  | Feedback                                           |
| `clients/web/src/components/ai-content-label.css`             | Attribution/feedback wiring is domain-owned; span flex layouts, text and button chrome are generic.                                                                                                                                                                             | Feedback                                           |
| `clients/web/src/components/ai-conversation.css`              | Transcript selection, role placement and foreground permission overlay are domain interactions. Embedded heading, model metadata, notices, empty/error states, activity rows and composer actions remain generic. Preserve foreground scrolling and message selection behavior. | Notes/editors; dialog overlap                      |
| `clients/web/src/components/ai-tool-settings.css`             | Provider policy is domain-owned; stacks, prose, empty state and form layout are generic. Asymmetric model/effort tracks need explicit retention or redesign.                                                                                                                    | Settings/forms                                     |
| `clients/web/src/components/app-empty-state.css`              | Entire stylesheet duplicates empty feedback and centered layout.                                                                                                                                                                                                                | Current                                            |
| `clients/web/src/components/app-error.css`                    | Message lifetime/dismiss behavior belongs to app; toast position/frame/action chrome is generic.                                                                                                                                                                                | Feedback                                           |
| `clients/web/src/components/app-shell.css`                    | HS2-6AM35F: app frame rules (presentations, focus ring, workspace scroller, safe-area, scrim) all justified; no Kerf equivalent.                                                                                                                                                | Shell/project                                      |
| `clients/web/src/components/attachment-context-menu.css`      | One-pixel fixed pointer anchor, no ordinary content styling. Check supported PopupMenu context opening before retaining.                                                                                                                                                        | Attachments                                        |
| `clients/web/src/components/attachment-gallery.css`           | Annotation boxes/handles, timed markers and media scaling are owned. Counts, playback/volume actions, popup and footer rows are generic. Filename foreground remains an exact API blocker.                                                                                      | Attachments; KF-7288MD                             |
| `clients/web/src/components/bulk-ticket-dialog.css`           | Tag-choice chrome and error text are generic; choices/mutations are domain state.                                                                                                                                                                                               | Dialogs                                            |
| `clients/web/src/components/command-navigation.css`           | HS2-SAXCMZ: 3 justified rules (public ListItem tokens on wrapper, filled tone, type-glyph dimming); palette color is set inline.                                                                                                                                                | Navigation                                         |
| `clients/web/src/components/command-run-dialog.css`           | Preformatted command output scrolling is owned; dialog, title, summary and action chrome are generic.                                                                                                                                                                           | Dialogs                                            |
| `clients/web/src/components/command-settings-editor.css`      | HS2-F9ECMZ: heading actions wa-button, blank EmptyState; rest tracked by HS2-7BBRJT (list), HS2-WNXRG3 (form), HS2-EBY4E3 (swatches/heading).                                                                                                                                   | Settings/forms                                     |
| `clients/web/src/components/confidence-badge.css`             | Score bands are domain mapping; compact rounded status chrome duplicates Badge.                                                                                                                                                                                                 | Metadata                                           |
| `clients/web/src/components/confidence-calibration.css`       | Multi-column statistics table and progress contract justify table CSS. Headings, recent results, stacks/text and band labels are generic.                                                                                                                                       | Settings/forms                                     |
| `clients/web/src/components/connection-details-dialog.css`    | HS2-D54GN5: heading/guidance now Kerf Text in List; 8 justified rules remain (popover size, body inset, revision code, embedded, ValueTable layout tokens pending KF-BTMDH5).                                                                                                   | Dialogs                                            |
| `clients/web/src/components/connection-state-banner.css`      | Animates a hook on Kerf LucideIcon; replace connecting icon with LoadingSpinner.                                                                                                                                                                                                | Current                                            |
| `clients/web/src/components/content-transition.css`           | Simultaneous sides, directional push/crossfade and inactive pointer policy are owned transition behavior. Action-side flex layout can compose Row.                                                                                                                              | Notes/editors; retain transition mechanism         |
| `clients/web/src/components/conversation-export-dialog.css`   | Export choices/collisions are domain state; dialog, section, radio choices, error and layout chrome are generic.                                                                                                                                                                | Dialogs                                            |
| `clients/web/src/components/corrupt-ticket-row.css`           | Recovery selection and shake feedback are owned. Row/header/body/action/prose/code framing are generic.                                                                                                                                                                         | Ticket presentations                               |
| `clients/web/src/components/cursor-semantics.css`             | Kerf's document baseline owns native and disabled wa-button cursors (HS2-CYSE1W); this keeps only data-action option rows and resize-drag cursors.                                                                                                                              | Retain; inspect controls during migrations         |
| `clients/web/src/components/drive-control.css`                | Removed by HS2-TQDZET (Kerf SplitButton). Was HS2-KM90NF: 8 justified rules, no !important; awaits a Kerf split action (KF-KFC7C2). Contrast bug HS2-9G0Y8W.                                                                                                                    | Shell/project                                      |
| `clients/web/src/components/drive-options-menu.css`           | Removed (HS2-EG31YP); Kerf's context `PopupMenu` anchor replaced the shared wrapper (HS2-H4JZDQ).                                                                                                                                                                               | Shell/project                                      |
| `clients/web/src/components/flow-back-button.css`             | Back policy is owned; button and text/icon row geometry are generic.                                                                                                                                                                                                            | Shell/project                                      |
| `clients/web/src/components/hs1-migration.css`                | Migration state/progress are owned; intro/path/destination/actions/details and flat banner geometry are generic.                                                                                                                                                                | Dialogs/feedback                                   |
| `clients/web/src/components/keyboard-settings.css`            | Chord display, capture/conflict state and fixed shortcuts are owned. Groups/rows/prose/actions/control chrome are generic.                                                                                                                                                      | Settings/forms                                     |
| `clients/web/src/components/lucide-icon-picker.css`           | Icon catalog/selection are domain-owned; search field, wrapping choices, buttons and hint/empty prose are generic.                                                                                                                                                              | Settings/forms                                     |
| `clients/web/src/components/manual-model-dialog.css`          | Dialog width only; public DialogSurface size should own this.                                                                                                                                                                                                                   | Dialogs                                            |
| `clients/web/src/components/markdown-editor.css`              | Source height, reading scale, editing cursor and recovery-compatible editor behavior are owned. Outer surface, toolbar and button chrome are generic.                                                                                                                           | Notes/editors                                      |
| `clients/web/src/components/markdown-preview.css`             | Generated Markdown headings/lists/code/tables/images/quotes/attachment links justify renderer-owned CSS. Do not replace arbitrary Markdown with Text. Empty copy can use Text at the render boundary.                                                                           | Retain renderer; notes/editor empty boundary       |
| `clients/web/src/components/not-working-dialog.css`           | Feedback workflow is owned; form/text/textarea/footer/action chrome is generic.                                                                                                                                                                                                 | Dialogs                                            |
| `clients/web/src/components/note-card.css`                    | Inline replies, paragraph-target feedback and choices are owned. Card/header/time/actions/prose/editor/choice chrome are generic. The content '+' pseudo-icon also warrants Lucide replacement.                                                                                 | Notes/editors                                      |
| `clients/web/src/components/note-composer.css`                | Note creation behavior is owned; surface/textarea/footer layout are generic.                                                                                                                                                                                                    | Notes/editors                                      |
| `clients/web/src/components/notification-center.css`          | Pending/history grouping is owned; queue stacks and empty surface are generic. Retain page maximum width only with justification.                                                                                                                                               | Current                                            |
| `clients/web/src/components/notification-inspector.css`       | Empty inspector height/background; public pane/list fill owns this geometry.                                                                                                                                                                                                    | Current                                            |
| `clients/web/src/components/notification-navigation.css`      | Footer grid and inset only; List/ListInsetControl.                                                                                                                                                                                                                              | Current                                            |
| `clients/web/src/components/notifications-paused-banner.css`  | Domain pause state; flat full-width StateBanner styling needs exact public configuration rather than root overrides.                                                                                                                                                            | Feedback                                           |
| `clients/web/src/components/pending-attachment-picker.css`    | File chooser hiding/drop state are owned; heading/rows/names/remove buttons/drop copy are generic.                                                                                                                                                                              | Attachments                                        |
| `clients/web/src/components/permission-request-card.css`      | Decisions/countdown and popup anchor/layers are owned; header/metadata/status/buttons/footer are generic. Preserve foreground popup scroll behavior.                                                                                                                            | Feedback                                           |
| `clients/web/src/components/project-close-dialog.css`         | Terminal preview aspect sizing is owned; intro/list-detail panes/empty/prose/error/actions are generic.                                                                                                                                                                         | Dialogs                                            |
| `clients/web/src/components/project-dialog.css`               | Path/recovery/provider choices are owned; form layout/prose/recovery banner/remote rows are generic.                                                                                                                                                                            | Dialogs                                            |
| `clients/web/src/components/project-setup-warning-banner.css` | Setup state is owned; hook-based flat StateBanner geometry needs public configuration.                                                                                                                                                                                          | Feedback                                           |
| `clients/web/src/components/project-sidebar.css`              | HS2-Q5ZZFF: DriveControl grid track and unread Badge anchor remain; the chat toggle chrome is Kerf Button.                                                                                                                                                                      | Shell/project                                      |
| `clients/web/src/components/project-statistics.css`           | HS2-3V3JY6: removed; Pane, kui-content and Text compose the page.                                                                                                                                                                                                               | Shell/project                                      |
| `clients/web/src/components/project-summary.css`              | HS2-H0THQW: domain chart CSS, every rule justified; counts use Kerf Text; compact size is one rule.                                                                                                                                                                             | Shell/project; retain chart                        |
| `clients/web/src/components/project-tab-bar.css`              | HS2-VMHN64: 4 justified strip rules (public TabBar tokens, surface/divider variants, mobile centering); selected-value layout uses Kerf Row/Text.                                                                                                                               | Shell/project                                      |
| `clients/web/src/components/project-tab-context-menu.css`     | One-pixel pointer anchor only; check PopupMenu context API.                                                                                                                                                                                                                     | Shell/project                                      |
| `clients/web/src/components/project-tab.css`                  | HS2-TNC9QK: indicator cluster uses Kerf Row; remaining rules are justified app-owned indicators (activity ring, counts, operation) and TabBarItem public tokens.                                                                                                                | Shell/project/metadata                             |
| `clients/web/src/components/provider-icon.css`                | Exact external provider brand SVG dimensions justify branded-media adapter CSS; Lucide does not replace provider logos.                                                                                                                                                         | Retain                                             |
| `clients/web/src/components/provider-setup-form.css`          | Device auth/reuse/accounts/permissions are owned; form stacks/labels/controls/account rows/notices/actions are generic.                                                                                                                                                         | Settings/forms                                     |
| `clients/web/src/components/quick-ticket-composer.css`        | Ticket creation/drop state/details autosize are owned; dialog/form/buttons/metadata/attachment rows/status/footer are generic.                                                                                                                                                  | Dialogs; attachments/notes overlap                 |
| `clients/web/src/components/repository-setup.css`             | Repository initialization workflow is owned; message/icon tile/error/footer/grid are generic.                                                                                                                                                                                   | Settings/forms                                     |
| `clients/web/src/components/repository-summary.css`           | HS2-B73RAF: one public-token rule remains (label weight); icon colors use LucideIcon color, branch name is a truncating Kerf Text.                                                                                                                                              | Navigation                                         |
| `clients/web/src/components/saved-view-dialog.css`            | Width/error color only; public DialogSurface/Text.                                                                                                                                                                                                                              | Dialogs                                            |
| `clients/web/src/components/server-busy-bars.css`             | Animated bars/status lifetime are an owned indicator; floating status copy/chrome is generic feedback. Evaluate spinner substitution against the product contract.                                                                                                              | Feedback                                           |
| `clients/web/src/components/settings-navigation.css`          | Generic pane content padding/gap.                                                                                                                                                                                                                                               | Current                                            |
| `clients/web/src/components/settings-workspace.css`           | Permission policy is owned; page width/inset/options/grids/explanatory copy are generic.                                                                                                                                                                                        | Settings/forms                                     |
| `clients/web/src/components/status-badge.css`                 | Reduced (HS2-3FMK0T): Kerf Badge owns the pill and tones; only the interactive trigger button reset remains.                                                                                                                                                                    | Metadata                                           |
| `clients/web/src/components/tag-chip.css`                     | Removed (HS2-ET9109): TagChip renders Chip directly; its identity is Chip's `data-item-id`.                                                                                                                                                                                     | Metadata                                           |
| `clients/web/src/components/terminal-clipboard-dialogs.css`   | Selectable/read-only monospace content is clipboard behavior; dialog/hint/textarea framing are generic. Context anchor needs API check.                                                                                                                                         | Dialogs                                            |
| `clients/web/src/components/terminal-dashboard.css`           | Grid fitting/canvas/mirrors/previews/magnification/mobile keyboard geometry are owned. Headers/counts/footer controls/empty states/chat preview copy are generic.                                                                                                               | Terminal chrome                                    |
| `clients/web/src/components/terminal-drawer.css`              | Focus-mode placement/backdrop/keyboard coordination are owned; exit/text-size/clipboard controls/rail/tabs/status/dots are generic.                                                                                                                                             | Terminal chrome                                    |
| `clients/web/src/components/terminal-key-bar.css`             | Sticky modifiers, horizontal touch scrolling and locked-key state are real contracts; groups/button chrome maps to toolbar primitives.                                                                                                                                          | Terminal chrome                                    |
| `clients/web/src/components/terminal-operations-sidebar.css`  | Generic stacks/gutters/divider; chart remains child-owned.                                                                                                                                                                                                                      | Current                                            |
| `clients/web/src/components/terminal-ticket-rail.css`         | Rail domain composition is owned; heading/divider/fill and Select presentation tuning are generic.                                                                                                                                                                              | Terminal chrome                                    |
| `clients/web/src/components/terminal-visibility-dialog.css`   | Visibility/name/context actions are app state; dialog/tabs/rows/footer/buttons are generic.                                                                                                                                                                                     | Dialogs                                            |
| `clients/web/src/components/textarea-sizing.css`              | HS2-V44W9T: removed; the coarse-pointer textarea tokens moved into hot-sheet-tokens.css.                                                                                                                                                                                        | Retain                                             |
| `clients/web/src/components/ticket-attachments.css`           | Batch editing/reordering, drag targets, file chooser hiding and thumbnails are owned. Headings/counts/rows/menus/select/browse/drop copy/batch framing are generic.                                                                                                             | Attachments                                        |
| `clients/web/src/components/ticket-board-column.css`          | Scroll snap/drag targeting are owned; header/count/stack/progress/scroll framing are generic.                                                                                                                                                                                   | Ticket presentations                               |
| `clients/web/src/components/ticket-board.css`                 | Runtime column sizing and paged snap are domain layout. Ordinary grid/empty centering can compose Grid/List; preserve horizontal variable-width contract.                                                                                                                       | Ticket presentations                               |
| `clients/web/src/components/ticket-close-dialog.css`          | Duplicate-search workflow is owned; dialog/results/selected summary/hint/error/actions are generic.                                                                                                                                                                             | Dialogs                                            |
| `clients/web/src/components/ticket-code-review.css`           | Commit graph/range/git refs are domain data; evidence cards/compare banner/rows/actions/ref pills/empty messages are generic.                                                                                                                                                   | Ticket presentations                               |
| `clients/web/src/components/ticket-duplicate-backlinks.css`   | Duplicate relationships are domain mapping; stacks/row copy/warning prose are generic.                                                                                                                                                                                          | Ticket presentations                               |
| `clients/web/src/components/ticket-field-conflict.css`        | Version comparison/merge input are app behavior; warning surface/header/grid/labels/output/actions are generic.                                                                                                                                                                 | Notes/editors                                      |
| `clients/web/src/components/ticket-info-panel.css`            | Ticket editing/feedback/provenance are owned; metadata grid/status row/details and blocked surfaces/labels/feedback header are generic.                                                                                                                                         | Notes/editors                                      |
| `clients/web/src/components/ticket-inspector-panel.css`       | Reader/sidebar contracts matter; stack/inset/overflow/child margin normalization need public List/Pane composition.                                                                                                                                                             | Ticket presentations                               |
| `clients/web/src/components/ticket-inspector-placeholder.css` | Entire stylesheet duplicates empty feedback/centering.                                                                                                                                                                                                                          | Current                                            |
| `clients/web/src/components/ticket-inspector.css`             | Title ghost autosize/reading scale/drag state are owned; frame/header/prose/banner/outcome/confidence/tabs/count presentation are generic.                                                                                                                                      | Ticket presentations                               |
| `clients/web/src/components/ticket-link-choice-dialog.css`    | Qualified source selection is domain policy; dialog/rows/source metadata/footer are generic.                                                                                                                                                                                    | Dialogs                                            |
| `clients/web/src/components/ticket-list.css`                  | Diagnostic/ticket sequencing/empty fill are owned; stacks/list surface/progress copy are generic.                                                                                                                                                                               | Ticket presentations                               |
| `clients/web/src/components/ticket-notes.css`                 | Notes/loading policy is owned; stacks/empty inset/placeholder card/header/body are generic.                                                                                                                                                                                     | Notes/editors                                      |
| `clients/web/src/components/ticket-page-more.css`             | Pagination state is owned; whole button chrome is generic.                                                                                                                                                                                                                      | Ticket presentations                               |
| `clients/web/src/components/ticket-reader.css`                | Reader stacking/transitions/large-text preference are owned; dialog shell/layer context header/text/layout are generic.                                                                                                                                                         | Dialogs/ticket presentations                       |
| `clients/web/src/components/ticket-row-context-menu.css`      | One-pixel context anchor/icon wrapper; use PopupMenu context API if exact contract fits.                                                                                                                                                                                        | Ticket presentations                               |
| `clients/web/src/components/ticket-row.css`                   | Dense summary/completion emphasis/selection grouping/cut and drop state/list-column parity are real contracts; surfaces/rows/metadata/feedback/status/actions are generic.                                                                                                      | Ticket presentations                               |
| `clients/web/src/components/ticket-search-field.css`          | Suggestions/date/help semantics are owned; fields/prose/help action/popup body are generic. Grouped anchoring remains an exact API blocker.                                                                                                                                     | Settings/forms or search-specific scope; KF-M8SV15 |
| `clients/web/src/components/ticket-source-setup-dialog.css`   | Provider/scope/removal/remote workflow is owned; dialog/options/metadata/remote notice/form/progress are generic.                                                                                                                                                               | Dialogs                                            |
| `clients/web/src/components/ticket-sources-settings.css`      | Provider/account capabilities are owned; heading/rows/groups/metadata pills/errors/account empty state are generic.                                                                                                                                                             | Settings/forms                                     |
| `clients/web/src/components/ticket-status-menu.css`           | App-owned badge and fixed context-menu anchor around Kerf PopupMenu; CSS sets wrapper display/width and anchor geometry only.                                                                                                                                                   | Metadata                                           |
| `clients/web/src/components/ticket-tag-editor.css`            | Only tag chip-row spacing and the anchored surface's width remain app-owned; Kerf owns the Combobox field and hint (HS2-C32ERK).                                                                                                                                                | Notes/editors                                      |
| `clients/web/src/components/ticket-timeline.css`              | Timeline connector/dot geometry/chronology are owned; heading/time/label/summary and row layout are generic.                                                                                                                                                                    | Ticket presentations                               |
| `clients/web/src/components/trash-settings.css`               | Trash policy is owned; field/page bounds need justification; status typography is generic.                                                                                                                                                                                      | Settings/forms                                     |
| `clients/web/src/components/view-navigation.css`              | HS2-78TFSM: two public-token rules remain (drag target, Errors danger tone); context anchor and search-count layout now compose Kerf's context PopupMenu anchor (HS2-H4JZDQ) and Row.                                                                                           | Navigation                                         |
| `clients/web/src/components/workspace-header.css`             | HS2-8G5C1P: Empty Trash uses wa-button start slot; remaining rules justified; star pressed-chrome override pending KF-GGDABA.                                                                                                                                                   | Shell/project; KF-MXE9YV                           |
| `clients/web/src/hot-sheet-tokens.css`                        | Owned theme/domain palettes, touch minimum and reading scale justify configuration. Review copied spacing defaults against foundation while preserving intentional rhythm.                                                                                                      | Retain configuration                               |
| `clients/web/src/style.css`                                   | Duplicate document defaults are current scope; safe-area bridge/viewport clipping may remain. Toast/loading chrome belongs to feedback follow-up.                                                                                                                               | Current/feedback                                   |
| `clients/web/src/ux-demo/style.css`                           | Stage bounds/fixture frames/viewport sizes/resize handles/context surfaces are legitimate. Many captions/rows/grids/forms/buttons/settings inspector/empty messages are generic. Stage CSS must not own production behavior or restyle nested components.                       | Development/catalog UI                             |

## Verification expectations

Read the consumer catalog and installed per-component pages/signatures before implementation. Update demo entries and regenerate the consumer catalogs when reusable compositions change. Remove obsolete styleSources and narrow profile exceptions; never increase doctor budgets to accommodate migration errors.

Existing relevant coverage includes component unit tests, `tests/ux-demo.spec.ts`, `tests/providers.spec.ts`, `tests/project-startup.spec.ts`, and `tests/terminal-dashboard-startup.spec.ts`. Source/CSS-string assertions must be replaced with public composition and behavioral assertions when old markup disappears. Exercise production parents as well as isolated demos, preserve delegated actions and accessibility contracts, inspect wide/narrow rendered states and transitions, and attach representative screenshots before claiming visual completion.

This is an ownership and migration inventory, not evidence that the follow-ups have been implemented or visually verified.

## Dialog migration update (HS2-XB41PF)

The preceding 103-file manifest records the original audit baseline. This migration removes
`clients/web/src/components/bulk-ticket-dialog.css` and
`clients/web/src/components/ticket-link-choice-dialog.css`: their generic copy, choices, sizing,
and actions now use public compositions. `project-close-dialog.css` retains raw Web Awesome viewport
geometry and owned asymmetric preview/canvas layout; warning, resource stack/metadata, prose, and
footer styling now comes from public Kerf components. Its preview shell is deliberately not wrapped
in `DialogSurface`, whose beta.76 props lack the modal gutter/max-height configuration required to
preserve the existing phone preview behavior. `KF-E2J9ND` requests the missing configuration, and
`HS2-FXAAA6` tracks downstream adoption. No Kerf component root or child styles are overridden.

## Metadata migration update (HS2-PQHN1J)

The metadata scope's stylesheets were already gone or reduced by earlier tickets:
`confidence-badge.css` (HS2-MBFKJF), `tag-chip.css` (HS2-ET9109), and `ticket-status-menu.css`
(HS2-X2AGAE) are removed, and `StatusBadge` composes Kerf `Badge` (HS2-3FMK0T). This update removes
the remaining wrapper markup that only existed because Badge lacked props:

- `ConfidenceBadge` renders Kerf `Badge` as its root. `KF-2YKKK5`'s `icon`, `title`, `label`
  (labeled image), and `rootAttributes` carry the gauge icon, explanation, accessible name, and
  `data-confidence-badge`/`data-band`/`data-confidence` identity. The former wrapper also inflated
  the pill's line box (23px instead of Badge's 20px compact height); the Kerf root restores Badge's
  own geometry. The gauge icon uses Badge's 1em icon box.
- `BlockedBadge` renders Kerf's solid `Badge` directly, identified by `data-blocked-badge`.
- `TagChip` already renders Kerf `Chip` directly; nothing remains.
- `TicketStatusMenu` keeps Kerf `PopupMenu` because its Started item has a nested phase submenu that
  `Select` cannot express. `status-badge.css` keeps only the native trigger button's UA reset and pill
  focus ring: app-owned native-button CSS, not a Kerf override. `KF-PHGQ6D` requests a public Badge
  menu trigger, and `HS2-M414Z9` removes the stylesheet when it ships.

## Attachment controls update (HS2-VHY325)

- `attachment-context-menu.css` was already removed, and `PendingAttachmentPicker` already used
  Kerf `FilePickerTrigger`/`FileDropZone` (HS2-RB7KXZ).
- `TicketAttachments` now renders its Add trigger and drop zone as Kerf `FilePickerTrigger` and
  `FileDropZone`, routed through the shared `routeFilePicker` under the `ticket-attachments` picker
  id. The browse/drop label rules and the hidden file-input rule are gone from
  `ticket-attachments.css`. The surface-wide drop stays app-owned and skips drops the zone already
  reported. Files dragged over the surface still highlight the zone through its documented
  `data-dragging` wiring state; `KF-HNRWM9` requests a public drop scope, and `HS2-P413MB` adopts it.
- Remaining generic chrome is split into its own tickets: attachment rows, row menu button, batch
  header, and regroup target (`HS2-K9HSXZ`); AttachmentGallery controls (`HS2-5B8FMA`). Thumbnails,
  annotation markers, media scaling, and drag/regroup state stay app-owned.

## Attachment rows and batch header update (HS2-K9HSXZ)

- Rows are a neutral Kerf `ContentItem` around a `Row`, a truncating `Text` name, and a plain
  `IconButton` More Actions trigger. An unstyled app-owned wrapper keeps `draggable` and the
  `ticket-attachment-item` identity, which Kerf root attributes protect.
- Batch titles are large Kerf `Text` headings that still wrap long labels, with a plain
  `IconButton` Edit action; double-clicking the heading stays a shortcut. The label editor is a
  small `wa-input` (Escape waits for it to render the restored label before blurring). The purpose
  is a caret-free inline Kerf `Select` with an info `Badge` selection; it is no longer a native
  select, so its iOS zoom rule is gone. The New group target is an info `ContentItem`.
- `ticket-attachments.css` keeps app-owned rules only: the wrapper cursor, batch/New group
  drag-over outlines, and the media grid and annotation marker.
- Upstream requests: `IconButton` has no hover `title`, so the More Actions tooltip is gone
  (`KF-8MYB8B`); `Text` has no heading size matching the former 16px semibold title, and
  `ListHeader` cannot wrap, so titles are 20px (`KF-43XH56`). `HS2-D087X2` adopts both.

## Attachment gallery update (HS2-5B8FMA)

- The `!important` `.attachment-gallery__pressed` override is gone. Toggles set `aria-pressed`, and
  their `ToolbarControlGroup`s use `selectedChrome="filled"` with `selectedTone="brand"`. The app's
  own toolbar and footer wrappers map the brand fill to the loud pair through public
  `--kui-color-brand-*-normal` tokens. The matching `KUI-L008` profile exception is removed.
- The annotation count is a solid info `Badge`; its app span only pins it to the toggle corner.
- Notes-rail prompts, the crop hint, and the volume label are Kerf `Text`, so the `> p`, crop-hint
  font, and volume-label rules are gone. The note field label has its own class, so the Closed outline
  toggle no longer needs `display: flex !important`.
- Retained as app-owned: the full-screen shell and dark palette, media stage and scaling, the
  annotation overlay, handles and badges, the timeline track, ticks, and range handles, the native
  note textarea and its iOS floor, the selected-note quote, and the counter's tabular width.
  `KF-7288MD` (filename foreground) stays counted.

## Provider and ticket-source settings update (HS2-EN1E76)

- App Settings → Accounts renders each sign-in as a base-surface Kerf `FramedList`, with a header
  row and one row per source. Its header, source, empty-note, divider, actions, and host rules are
  gone. One wrapper rule zeroes the public `--kui-layout-inline-margin` token so the icons stay on
  the settings text edge (HS2-1JSPX7).
- The GitHub repository and attachment-assets fields are Kerf `Combobox`es instead of styled native
  datalist inputs, so the `__field`, `__field-control`, `__field-hint`, and iOS-zoom rules are gone.
- Retained: the project Ticket sources and the reuse-account cards keep their bordered `List` of
  actionable `ListItem`/`ListActionRow` rows. FramedList is not for navigation rows, and `List
frame` insets its rows. Also retained are the phrasing-only copy grids inside row labels, the
  device-code chip, the inline repository-access links, the settings text-link button, and the
  Grid full-width placement class.

## Command, keyboard, icon-picker, calibration, and search-help update (HS2-4GX2YS)

- KeyboardSettings renders each shortcut group as a base-surface Kerf `FramedList`. The recording
  row is its `highlighted` row, and the list, row, divider, and capturing rules are gone (the old
  `KF-2HB7RT` gap). A wrapper zeroes the public inline-margin token, so shortcut names stay on the
  settings text edge.
- The ticket search form field is a Kerf `List`, so its grid rule is gone. The command editor's
  status message is Kerf `Text`; its footer keeps only the reserved line height.
- Retained:
  - The keycap chord and capture control, which are owned interaction state.
  - The confidence calibration numeric table and its semantic `ul` of recent completions. Kerf
    layout roots cannot carry `listitem`.
  - The command editor's semantic grouped `ul`, drop targets, dialog icon, and fieldset legend.
  - The search help's inline `code` chips and the date helper's responsive tracks.
- The icon picker's square 36px toggles wait on `KF-K60RRM`; `HS2-XA11C3` adopts it.

## Notes, conversation, and editor update (HS2-ZA1EAG)

- TicketNotes stacks its header, cards or empty line, composer, and add row in a Kerf `List`. Only the
  section's `min-width` remains. The render is pixel-identical at 1280 and 390px.
- The AIConversation message meta row is a Kerf `Row` (space-between, wrap). Its footer keeps only
  the offset from the message.
- The Add tag field uses Kerf `Combobox` inside `AnchoredSurface`, with KF-C558Z9's top-layer
  suggestion listbox. One shared application wiring routes tag and provider repository commits;
  the field's app-owned CSS and coarse-pointer font rule are gone (HS2-C32ERK).
- The remaining generic chrome is split into its own tickets: NoteCard (`HS2-CDPB73`), AIConversation
  (`HS2-4RXG6W`), and MarkdownEditor, TicketInfoPanel surfaces, and TicketFieldConflict versions
  (`HS2-26E5P7`).

## Shell and project chrome update (HS2-TQDZET)

- `drive-control.css` is removed. DriveControl renders Kerf `SplitButton` (the old `KF-KFC7C2` gap):
  success fill while idle, neutral fill while running, with its `drive-control` identity on an
  unstyled wrapper. The `KUI-L004` profile exception for its options half is gone.
- The project work summary is centered Kerf `Text` (`align="center"`), so its CSS rule is gone.
  The test hook is now `data-work-summary`.
- The project sidebar chat toggle is a pressed Kerf `Button` (formerly `IconToggleButton`),
  with Kerf's disabled `not-allowed` cursor (KF-HJD6XB, HS2-Q5ZZFF). The app CSS retains
  only the Drive row and the unread Badge anchor.
- Retained: the ProjectSummary chart tile (bar geometry), the project tab, tab bar, and close or
  open dialog geometry, the app shell safe areas, and the workspace header rules recorded in their
  manifest rows. Further shell, tab, and header chrome is tracked in `HS2-9TH4AC`.

## Terminal chrome update (HS2-1Q6GER)

- `TerminalDashboard` empty, loading, and message copy now composes Kerf `List` and `Text`. The
  `.terminal-dashboard__empty` rules are gone, and the `/ux-demo` entry exposes all three states plus
  a short drawer grid. The controls wrapper is a Kerf `Row`, so `.terminal-dashboard-controls` is
  removed. Since Kerf beta.106 (`KF-3ZJ9PF`) one filled `List` gathers the title and copy in its
  middle, so the former inner `List` workaround is gone (HS2-BH7NZ4).
- Retained with exact upstream requests: the visibility Select and ticket-rail view-title width
  bounds (`KF-DQANHZ`), the hidden-item corner count (`KF-WT2EMZ`), and the drawer tab
  status-icon wrappers that carry a hover title (`KF-2V9XJQ`). `HS2-9J6MYJ` adopts those
  shipped APIs.
- Owned contracts that stay: the key bar's sticky Fn group, touch scroller, and safe-area backing;
  focus-mode placement and backdrop; grid fitting, canvas, previews, magnification, and mobile
  keyboard geometry. Generic tile identity and footer typography remain in `HS2-9J6MYJ`.

## Development and catalog UI update (HS2-3AZ08H)

- The Dev Review tool and its stylesheet were already removed (HS2-03PV1C), and the earlier
  CatalogExample migrations retired most demo stage CSS.
- `ux-demo/style.css` drops its generic rules:
  - Every entry's settings form is `DemoSettingsForm`, a Kerf `List` with the standard gap and 8px
    control insets (24 forms).
  - The Project Settings stand-in is `DemoSettingsPreview`, a framed Kerf `ContentItem` surface of
    `Text` at the reading inset.
  - The AppShell workspace and terminal-drawer placeholders are Kerf `EmptyState`s.
  - `.settings-form`, `.workspace-settings-preview*`, and `.shell-mode-surface*` are gone.
- Retained as legitimate catalog stage CSS:
  - The theme/contrast/reduced-motion display controls.
  - The project-sidebar resize cursor.
  - The `demo-tone-surface` inherit/inverse color contexts (CatalogExample surfaces cover only the
    default and lowered fills).
  - The phone-sized terminal focus stage.
    None of them restyles a nested production component.

## Settings and forms update (HS2-J7BTAG)

- `ai-tool-settings.css` was already removed.
- `trash-settings.css` is removed. A Kerf `List` flex item (`flex(0, 1, px(320))`) bounds the
  retention field and lets a narrow screen wrap the action below it.
- `RepositorySetup` renders its initialize lead as centered quiet Kerf `Text`, so the
  `.repository-setup__lead` rule is removed.
- The page-column bounds stay: the `settings-workspace.css` 864px measure and the
  `repository-setup.css` 480px panel. `KF-P3069N` requests a public `List` content width, and
  `HS2-0ZF0KS` adopts it. The ready-icon disc stays, because no Kerf icon presentation draws a
  filled circle.
- The remaining generic chrome is split into its own tickets:
  - Provider setup and ticket-source settings: `HS2-EN1E76`.
  - Command, keyboard, icon-picker, calibration, and search-help forms: `HS2-4GX2YS`.
