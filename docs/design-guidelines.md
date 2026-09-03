# Design guidelines — Apple Human Interface Guidelines for Hot Sheet 2

> **Status: Decided** (maintainer direction, 2026-09-03; HS2-ZC24BS). Hot Sheet 2
> follows Apple's macOS Human Interface Guidelines (HIG, the macOS Tahoe / Liquid
> Glass edition, pages dated through June 2026) for the native macOS app, and adopts
> the same guidelines for the web/Tauri client wherever a browser can reasonably
> honor them. This document distills the HIG for this project — especially for AI
> tools that build the clients — and states how Hot Sheet leverages it now and as
> the clients grow. Source pages: [Designing for macOS](https://developer.apple.com/design/human-interface-guidelines/designing-for-macos)
> plus every Foundations, Patterns, Components, Inputs, and Technologies page
> (157 pages; see §12).

Related docs: [06-clients.md](06-clients.md) (client requirements),
[ux-components.md](ux-components.md) (component contract and `/ux-demo` catalog),
[09-technology-decisions.md](09-technology-decisions.md) §9.5 (Kerf + Web Awesome,
Lucide), [18-dev-review-tool.md](18-dev-review-tool.md), and the root
[CLAUDE.md](../CLAUDE.md) "Client UI stack" rules.

---

## 0. How to use this document

**Who reads it.** Anyone changing client UI — humans and AI coding tools alike. It is
written so an AI tool can apply it without opening Apple's site: every section ends in
rules, and §10 is a checklist.

**Precedence.** When sources disagree:

1. Explicit Hot Sheet rules in [CLAUDE.md](../CLAUDE.md) and the numbered requirement
   docs (for example: Lucide-only iconography, 150 ms autosave without Save/Cancel,
   no interval polling, the 1024×600 window floor). §9 lists every place these
   deliberately override or extend the HIG.
2. This document.
3. The HIG itself (for anything not covered here — link to the page in the ticket).

**Terms.** "macOS app" means the native SwiftUI client (client sequence step 3).
"Web client" means the Kerf + Web Awesome Core browser client, and "Tauri" the desktop
host that wraps it on macOS, Linux, and Windows (steps 1 and 2). "Follow the HIG"
means: adopt the HIG rule as written. "Translate" means: keep the intent and
implement it with web primitives. "Do not imitate" means: the web client must not
fake a native macOS surface it cannot actually be.

**Reading order for a task.**

| You are about to… | Read |
|---|---|
| Add or change any client surface | §1 (scope), §10 (checklist), then the matching §2–§6 section |
| Build a menu, toolbar, context menu, or shortcut | §4.3–§4.5, §5.1, §8.1 |
| Present a dialog, popover, alert, sheet, or panel | §4.7 decision table, §3.4 |
| Show AI output (notes, repairs, narration, permission requests) | §6.1, §3.12, §9 |
| Touch color, type, spacing, icons, dark mode | §2.1–§2.7, §9 |
| Start the SwiftUI macOS app | §8.1 (the whole native plan) |
| Work on Tauri | §8.2 |

---

## 1. Decision and scope

### 1.1 What we adopt, per client

**Native macOS app (SwiftUI): full HIG conformance.** Use system components
(`NavigationSplitView`, `Table`, `List`, `Menu`, `.toolbar`, `.inspector`, `.sheet`,
`.alert`, `.popover`, `Settings`, `WindowGroup`, `UNUserNotificationCenter`, App
Intents, WidgetKit) so Liquid Glass, materials, vibrancy, focus, accent color, Dark
Mode, Full Keyboard Access, VoiceOver, window states, and the menu bar behave
correctly with no custom chrome. Custom views exist only for Hot Sheet-specific
content (ticket rows, board cards, the terminal viewport, the activity timeline) and
must follow the same rules as the system views around them.

**Web client (browser and Tauri): HIG-shaped, not HIG-skinned.** Adopt the HIG's
structure, interaction model, behavior, writing, accessibility, keyboard conventions,
and presentation choices. Implement them with Web Awesome Core primitives styled
through their documented tokens and parts, and with Kerf-owned state. Do **not**
counterfeit native chrome: no CSS-drawn title bars or traffic lights, no imitation
Liquid Glass refraction, no SF Symbols (Lucide is the shared icon vocabulary), no
platform-specific cursor quirks that browsers do not have. On Linux and Windows the
same client keeps the HIG's *behavioral* rules (menus, undo, feedback, modality,
focus) with that platform's modifier keys and native window decorations.

**iOS/iPadOS (later):** out of scope for this revision. The digests already contain
the iOS platform considerations; HS2-46RA38 adds them before the iOS client starts.

### 1.2 Why

- A Mac-first product whose users live in the terminal and in native tools expects
  the menu bar, standard shortcuts, native windows, and honest feedback. The HIG is
  the most complete, maintained statement of those expectations.
- Two clients that share *conventions* (menu order, label grammar, undo naming,
  selection colors, dialog button placement, notification policy) feel like one
  product even though they share no rendering code — exactly the cross-client
  contract in [ux-components.md](ux-components.md) §1.
- The HIG's Generative AI and Machine learning pages are the best available
  guidance for a product whose whole purpose is surfacing AI work.

### 1.3 Applicability matrix

Ratings: **Adopt** (apply as written), **Translate** (keep intent, web primitives),
**Analogy** (borrow the principle only), **Skip** (not applicable on desktop).

| HIG area | macOS app | Web / Tauri | Where in this doc |
|---|---|---|---|
| Accessibility, VoiceOver, Full Keyboard Access | Adopt | Translate (ARIA, `:focus-visible`, `prefers-*`) | §2.1, §6.2 |
| Color, Dark Mode, materials, Liquid Glass | Adopt | Translate (tokens, `prefers-color-scheme`); no imitation glass | §2.2, §2.3, §2.6 |
| Typography (SF Pro, 13 pt body, 10 pt floor) | Adopt | Translate (system font stack, px scale) | §2.4 |
| Layout, split views, sidebar, inspector | Adopt | Translate (CSS grid, `wa-split-panel`) | §2.5, §4.2 |
| Icons (SF Symbols) | Decision pending (HS2-0P83KD) | Lucide (project rule) | §2.7, §9 |
| Motion | Adopt | Translate (`prefers-reduced-motion`) | §2.8 |
| Writing, inclusion, RTL | Adopt | Adopt | §2.9, §2.11 |
| Privacy, app icon, branding, images | Adopt | Translate | §2.10, §2.12 |
| Launching, loading, feedback, modality, undo | Adopt | Adopt | §3.1–§3.5 |
| Drag and drop, entering data, searching, settings | Adopt | Translate | §3.6–§3.9 |
| File management, full screen, multitasking | Adopt | Translate (Tauri) / Analogy (browser) | §3.10, §3.11 |
| Notifications, help, onboarding, accounts, sharing, charts, printing | Adopt | Translate | §3.12–§3.17 |
| Audio, haptics, video, workouts, live-viewing, ratings | Analogy | Skip | §3.18 |
| Windows, toolbars, menu bar, menus, context menus | Adopt | Translate (Tauri native menu; in-browser command surface) | §4.1, §4.3–§4.5 |
| Lists/tables/outline views, buttons, text controls, toggles, pickers, progress | Adopt | Translate (Web Awesome) | §4.6–§4.13 |
| Sheets, alerts, popovers, panels | Adopt | Translate (`wa-dialog`, `wa-popover`, `wa-drawer`) | §4.7 |
| Column views, page controls, ornaments, lockups, tab-bar-as-navigation | Skip | Skip | §4.14 |
| Notifications, widgets, App Intents, Dock menu, menu bar extra | Adopt | Tauri: notifications, tray | §4.12, §8.1 |
| Keyboards, pointing devices, focus and selection, gestures | Adopt | Translate | §5 |
| Other inputs (Pencil, Crown, eyes, remotes, game controls…) | Skip | Skip | §5.5 |
| Generative AI, Machine learning | Adopt | Adopt | §6.1 |
| Mac Catalyst, Siri/App Intents, iCloud, Sign in with Apple, SharePlay | Analogy / later | Analogy | §6.3–§6.4 |
| Remaining technologies (Pay, Wallet, HomeKit, Maps…) | Skip (principles noted) | Skip | §6.5 |

## 2. Foundations

Each subsection gives the HIG rule set, then **Hot Sheet application** (both
clients unless labeled).

### 2.1 Accessibility

- Support Full Keyboard Access: everything reachable and operable from the keyboard,
  without overriding system or accessibility shortcuts.
- Type: macOS default **13 pt**, minimum **10 pt**; avoid Ultralight/Thin/Light
  weights. Let people enlarge text (the HIG asks for up to 200%); macOS has no
  Dynamic Type, so never lock text sizes.
- Contrast: **4.5:1** for text up to 17 pt, **3:1** for 18 pt+ or bold; strive for
  **7:1** on custom pairs in Dark Mode; provide a higher-contrast variant when
  Increase Contrast is on; test light and dark with Increase Contrast and Reduce
  Transparency, separately and together.
- Controls: macOS **28×28 pt** default, **20×20 pt** minimum, roughly **12 pt**
  padding around bezeled elements and **24 pt** around bezel-less ones; buttons keep
  a **44×44 pt** hit region where possible.
- Never convey information with color alone; add a label, icon, or shape.
- No timer-dismissed UI; explicit dismissal only. Honor Reduce Motion (replace
  movement with fades, stop repeating animations). Nothing autoplays.
- Label every element for VoiceOver and Voice Control; hide decorative images.

**Hot Sheet application.** Ticket state (Needs review, blocked, Up Next, live
claim) always pairs its color or rail with text or an icon. Every icon-only control
has an accessible name and a tooltip. Toasts and notices never auto-dismiss without
a close control. Web: enforce WCAG AA on every `--wa-color-*-on-*` pair in
`clients/web/src/theme.css`, honor `prefers-reduced-motion`, `prefers-contrast`,
and `prefers-reduced-transparency`, keep `--wa-color-focus` rings visible, and give
Web Awesome controls ≥ 20 px (prefer 28 px) hit targets (HS2-DKZG9S, HS2-A2A9GT).

### 2.2 Color

- Use **semantic, adaptive colors**; never hard-code documented system values and
  never repurpose a semantic color (a separator color is not text; a secondary label
  color is not a background).
- Every custom color needs light, dark, and increased-contrast variants — even in a
  single-appearance app, so adaptive surfaces work.
- Liquid Glass has no color of its own; tint at most **one primary action** per
  surface (color the button background, not its label). Keep toolbars and menus
  monochrome when content is colorful.
- App accent color applies only while the user's system accent is "multicolor";
  otherwise the user's accent replaces it everywhere except deliberately fixed-color
  sidebar icons. Design so either looks right.
- macOS dynamic colors to use in the native app: `labelColor` … `quaternaryLabelColor`,
  `controlBackgroundColor`, `selectedContentBackgroundColor` (key window) vs
  `unemphasizedSelectedContentBackgroundColor` (inactive), `alternatingContentBackgroundColors`,
  `separatorColor`, `keyboardFocusIndicatorColor`, `windowBackgroundColor`,
  `controlAccentColor`, `linkColor`, `placeholderTextColor`.
- Offer the system color picker when people choose colors.

**Hot Sheet application.** Web: `--wa-color-*` tokens are the semantic model;
`--hs-*` aliases stay limited to domain states the library cannot name
(`--hs-ticket-state-needs-review`, `--hs-ticket-state-up-next`, `--hs-shell-divider`)
and each must gain a dark value (HS2-DKZG9S). One brand fill (`--wa-color-brand-fill-loud`)
is the only accent; do not tint multiple controls. Under Tauri on macOS, consider
following the OS accent color through a single accent token (design choice, not a
HIG requirement). Native: `Color(nsColor:)` semantic colors only; ticket-state colors
live in an asset catalog with all three variants.

### 2.3 Dark Mode

- No app-specific appearance setting; follow the system, including Auto switching
  while running.
- Dark is not an inversion: dimmer backgrounds, brighter foregrounds; use system label
  colors and system text controls so vibrancy is handled.
- Soften pure-white content images so they do not glow.
- macOS graphite accent enables desktop tinting; a custom component with a visible
  neutral background may include slight transparency so it harmonizes, but never in a
  colored state.

**Hot Sheet application.** The web client is light-only today; HS2-DKZG9S adds the
dark palette through `prefers-color-scheme` and the Web Awesome dark theme with no
in-app toggle. The Markdown preview, note cards, terminal drawer, and screenshots in
attachments are the surfaces to check for glowing whites.

### 2.4 Typography

- macOS system font is **SF Pro**; SF Mono for monospaced content. Do not embed
  system fonts; use `Font` designs.
- macOS built-in text styles (size/line height): Large Title 26/32, Title 1 22/26,
  Title 2 17/22, Title 3 15/20, **Headline 13/16 bold, Body 13/16**, Callout 12/15,
  Subheadline 11/14, Footnote 10/13, Caption 1 10/13, Caption 2 10/13 medium.
- Minimize typefaces; build hierarchy with weight, size, and color; keep the
  hierarchy when sizes change; loose leading for long passages, tight leading only
  for rows of at most two lines.
- Prefer multi-line labels over truncation where space allows; truncate identifiers
  in the middle so both ends survive.

**Hot Sheet application.** Native: `.body`, `.headline`, `.subheadline`,
`.caption`, `.monospaced` for slugs and paths. Web: HS2-PS29TA decides between the
current Inter-first stack and the HIG-aligned `-apple-system, BlinkMacSystemFont,
system-ui, "Segoe UI", sans-serif` stack with a 13 px UI base, a 10 px floor, four
weights (400/500/600/700), and `ui-monospace, SF Mono, Menlo` for slugs, ids, paths,
and the terminal. Never fix sizes in a way that defeats browser zoom.

### 2.5 Layout

- Group related items with space, backgrounds, materials, or separators; keep
  controls clearly distinct from content.
- Give essential information room; push secondary detail to the inspector or
  another view. Place by importance in reading order (top and leading first).
- Content extends edge to edge and continues **beneath** floating chrome (sidebar,
  toolbar, inspector) — the control layer floats over the content layer. Use a scroll
  edge effect, not an opaque bar background, at the content/control boundary.
- Align components; use progressive disclosure for hidden collections.
- Adapt: design the full layout first and defer compact layouts as long as
  possible; hide tertiary columns (the inspector) before the sidebar as width
  shrinks; test the largest and smallest layouts first.
- macOS: **never put critical controls or information at the bottom of a window**
  (people drag windows partly off-screen); avoid content under the camera housing.

**Hot Sheet application.** The window skeleton is sidebar · workspace (list or
board) · inspector, plus the bottom terminal drawer. The drawer is content, not the
sole home of any critical control; its toggle and actions live in the header and
View menu. The AppShell floor is **1024×600**; the collapse ladder at that width is
"inspector first, then sidebar", never a compressed layout. Web: extend list/board
backgrounds under a lightly translucent sidebar/inspector and use a hairline plus
subtle shadow that appears only once content scrolls beneath the header.

### 2.6 Materials and Liquid Glass

- Liquid Glass is the floating **control layer** (toolbars, sidebars, tab bars,
  menus, alerts, popovers). Never use it in the content layer (rows, cards,
  backgrounds); the one exception is a transient interactive control (slider, toggle)
  while it is manipulated.
- Use glass sparingly on custom controls; system components adopt it automatically.
  Regular glass (blurred, luminosity-adjusted) for text-heavy or legibility-sensitive
  surfaces; clear glass only over rich media, with a 35% dark dimming layer when the
  media is bright.
- Standard materials differentiate *within* the content layer; choose by semantic
  purpose, use vibrant colors on top, thicker for fine text, thinner to preserve
  context. macOS offers `NSVisualEffectView` materials with behind-window or
  within-window blending.
- Appearance changes with the user's Liquid Glass preference, Reduce Transparency,
  and Increase Contrast.

**Hot Sheet application.** Native: let `NavigationSplitView`, `.toolbar`,
`.inspector`, sheets, and menus supply glass; the terminal drawer, board columns,
and ticket rows are content and get standard materials at most. Web: approximate the
*layering* (one translucent treatment for navigation chrome with
`backdrop-filter`, opaque content surfaces, scroll-edge fade) and fall back to an
opaque `--wa-color-surface-raised` under `prefers-reduced-transparency`. Do not build
imitation refraction or specular highlights.

### 2.7 Icons and symbols

- One consistent family: same stroke weight, size, detail, and perspective; weight
  matched to adjacent text; optically centered; no baked-in selected states.
- Standard actions use standard metaphors everywhere they appear (Copy, Share,
  Delete, Search, Filter, Undo/Redo, Add, More, Attach, Rename, Move, Duplicate,
  Archive, Done, Cancel).
- Every custom icon has an accessibility description; decorative icons are hidden
  from assistive technology; direction-bearing icons flip for right-to-left,
  checkmarks, logos, and real-world objects do not.
- SF Symbols: monochrome or hierarchical rendering with semantic colors; outline in
  toolbars and menus, fill for selected states; animation sparingly and purposefully;
  symbols may not appear in app icons or logos.

**Hot Sheet application.** Web and shared vocabulary: **Lucide only** (project
rule), rendered through the shared icon component, `stroke-width` matched to text.
Maintain the standard-action map: `scissors`, `copy`, `clipboard-paste`, `check`,
`x`, `trash-2`, `undo-2`/`redo-2`, `square-pen`, `copy-plus`, `pencil`, `folder`,
`paperclip`, `plus`, `ellipsis`, `search`, `list-filter`, `share`, `printer`,
`circle-user`, `archive`, `calendar`, `panel-left`/`panel-right` (sidebar and
inspector toggles). Native: pending decision HS2-0P83KD; the recommendation is SF
Symbols in the native app with a maintained Lucide↔SF metaphor table so each
action keeps one meaning across clients.

### 2.8 Motion

- Purposeful, brief, precise, and optional: motion never carries information alone,
  never blocks interaction, and can always be interrupted.
- Do not add motion to frequent interactions; the system already animates standard
  components (and subdues Liquid Glass motion for pointer input).
- Feedback motion follows the gesture direction; a panel revealed from the right
  leaves to the right.
- Under Reduce Motion: fades instead of slides and zooms, no bounce, no repeating
  ambient animation.

**Hot Sheet application.** Kerf morph transitions stay around 150–250 ms and
non-bouncy. The live-claim "two-dot" activity indicator must stop animating under
Reduce Motion (keep the static indicator). Inspector and drawer open/close directions
match their close direction.

### 2.9 Writing and inclusion

- Decide a voice and keep a terms list; match tone to context.
- Buttons and menu items are verbs; never "Click here"; describe link destinations.
- Capitalization by element type, applied consistently: **title case** for menu
  items, menu titles, buttons, tab labels, column headings, alert titles that are
  fragments; **sentence case** for body text, alert titles that are sentences,
  tooltips, placeholders, box titles, settings labels (with a trailing colon for
  introductory labels in settings panes).
- Address people as "you"; never "the user"; never "we"; possessives sparingly
  ("Favorites", not "Your Favorites").
- Errors: prevent first; show next to the problem; say how to fix it; no blame, no
  "Oops"; if copy cannot fix a frequent error, redesign the interaction.
- Empty states welcome, explain, and offer the next action; crucial information never
  lives only in an empty state.
- Multi-step flows: "Get Started" … "Continue"/"Next" … "Done".
- Inclusive language: no unnecessary gender, no colloquialisms, no jargon without
  definition; internationalize dates, numbers, and strings from the start.
- Tooltips: 60–75 characters, verb-first fragment, sentence case, no ending
  punctuation, describe only the control under the pointer, do not repeat its name,
  and may change with control state.

**Hot Sheet application.** Ticket actions read "Start", "Complete", "Claim",
"Release", "Move to Up Next", "Mark Verified", "Report Not Working…". The empty
queue reads, for example, "No tickets are Up Next." followed by a New Ticket
action. AI-facing vocabulary (claim, lease, worker) is defined where it first
appears. Web copy says "click"; platform-specific key names come from the platform
(⌘ on macOS, Ctrl elsewhere).

### 2.10 Privacy

- Request only what a feature needs, when it needs it; precise purpose strings in
  sentence case ending with a period; a pre-alert screen has at most one
  "Continue" button.
- Keep secrets in the Keychain; never plain-text files; prefer passkeys, no custom
  authentication schemes.
- macOS: sign with Developer ID (notarize), consider App Sandbox, and never assume
  which user is signed in (fast user switching).

**Hot Sheet application.** Already the rule: only server instance data carries
bearer credentials; provider tokens are OS-keychain references; the browser never
holds secrets in page state. The native app will need precise purpose strings for
Files and Folders, local network, and notifications, and a sandbox decision that
accounts for spawning AI CLI tools.

### 2.11 Right to left and localization

- System components mirror automatically; custom layouts use logical properties.
- Numbers never reverse; progress and navigation controls flip; icons that show
  text direction or motion flip; logos, checkmarks, and real objects do not.
- A paragraph of three or more lines aligns to its own language.

**Hot Sheet application.** Low priority until an RTL locale is planned, but use CSS
logical properties (`margin-inline-start`, `inset-inline-end`, `text-align: start`)
and keep slugs in `dir="ltr"` spans now, so RTL becomes localization rather than a
redesign.

### 2.12 App icon, branding, images

- App icon: layered (background plus foreground layers) at **1024×1024 px**, square
  and unmasked, built in Icon Composer with default, dark, and mono annotations;
  simple, illustrative, no text, no UI screenshots, no Apple hardware, no baked-in
  effects.
- Branding defers to content: no logo in the working UI, no launch screen for
  branding, custom fonts only if they stay legible; an accent color that survives the
  user overriding it.
- Images: @1x and @2x for any raster on macOS; SVG/PDF for icons; embed color
  profiles; test on a non-Retina display.

**Hot Sheet application.** One Icon Composer source drives the native app, the
Tauri bundle (`.icns` and flattened PNG/ICO for Linux/Windows), and the web favicon.
Attribution is **Small Tale Inc.** everywhere (see CLAUDE.md "Project attribution").

---

## 3. Patterns

### 3.1 Launching and state restoration

- Launch instantly; macOS needs no launch screen and no splash.
- Restore exactly where people were: window frames, selected project, view mode,
  selected ticket, inspector tab, pane sizes, search text and filters — per window.
- Paint cached or skeleton content before the network connects.

**Hot Sheet application.** The web client already persists splitter sizes locally;
extend restoration to the full list above (per project tab) and keep the Tauri
window-state plugin for frames. Native: SwiftUI scene restoration.

### 3.2 Loading

- Show something immediately; a blank pane reads as broken. Let people keep working
  while content loads.
- Determinate progress when duration is known, indeterminate otherwise, switching to
  determinate as soon as possible; specific descriptions, never "Loading…".
- Keep previous results visible (dimmed) while a search or provider fetch refreshes.

**Hot Sheet application.** List/board first paint uses cached rows or skeletons;
index rebuilds, imports, and cross-store copies show "n of N" progress with Cancel;
search keeps the previous result set until the next arrives. Background
reconciliation never toggles the foreground loading surface (already the rule in
[06](06-clients.md) §6.3).

### 3.3 Feedback

- Match delivery to significance: passive, inline status for routine state; an alert
  only for critical, ideally actionable information.
- Warn before **unexpected and irreversible** loss; do not confirm expected outcomes
  (Finder does not confirm every Trash).
- Confirm completion only for significant actions; people assume success and need to
  hear about failure, with the reason.
- Every feedback channel has a non-visual or non-auditory counterpart.

**Hot Sheet application.** Autosave status is a passive "Saved"/"Saving…"/"Offline,
changes queued" text near the editor, never a toast per save. Connection and sync
state live inline in the sidebar/banner, with the reason when a store is
unreachable. Claim conflicts explain why in place ("Claimed by another worker").

### 3.4 Modality

- Present modally only for a clear benefit: a required decision, a narrow task, or
  confirmation of a just-taken action. Keep modal tasks short and linear; title them
  with the task.
- One modal at a time; never stack sheets; an alert may sit above another modal.
- macOS dismiss controls live in the content area (buttons at the bottom of a
  sheet); a separate window is an acceptable alternative for a distinct task.
- Nonmodal companions (inspector, drawer) never block the main view.

**Hot Sheet application.** Permission requests are queued and shown one at a time;
the non-modal permission popup stays reachable while other work continues. New
ticket, provider connection, bulk operations, and Not Working evidence are sheets
with Cancel plus a verb-named primary action. Editing is never modal.

### 3.5 Undo and redo

- Multi-level undo of everything since a logical step; label items with the result
  ("Undo Move to Done", "Undo Delete 3 Tickets"); reveal the affected content
  (scroll or select) so people do not undo twice.
- Edit menu, ⌘Z / ⇧⌘Z; toolbar undo buttons only when necessary.
- Batch related incremental changes into one entry.

**Hot Sheet application.** The web client already has a field-aware per-checkout
history ([06](06-clients.md) §6.1). Add result-describing names to every entry, wire
them to the Edit menu under Tauri and native, and make undo select and scroll to a
ticket that is off-screen or in another view. Text-level undo inside editors must
survive the 150 ms autosave. Offer undo only where the provider can honor it;
otherwise confirm before the action.

### 3.6 Drag and drop

- Same container = move; different container = copy; Option held **at drop time**
  forces copy. Support multi-item drags with a count badge on macOS.
- Feedback: translucent drag image after ~3 pt of movement; valid targets highlight
  one at a time; invalid targets show nothing or the not-allowed cue; failed drops
  animate back or evaporate; auto-scroll while dragging inside a scrolling target.
- Keep dropped items selected; deselect originals on cross-container drags.
- Every drag has a non-drag equivalent (menu command, keyboard).
- macOS: allow dragging from an inactive window without activating it; consider
  drags to Finder in a reopenable format.

**Hot Sheet application.** Ticket → board column or sidebar queue = move (status
change), undoable, keeps selection. Ticket → another store = copy by default, move
via Option or an explicit "Move to…" item, routed through the idempotent copy/move
operation. Attachments drop onto the inspector or composer with a placeholder row and
progress. "Move to…", "Copy to…", and "Set Status" remain in the context menu.

### 3.7 Entering data

- Prefer choosing over typing (pickers, menus, tokens); prefill defaults; read what
  the system already knows.
- Label every field and use placeholders for format; validate as people type or on
  focus loss as appropriate; enable the one explicit Create/Connect button only when
  required data is present.
- Secure fields for secrets; never prefill a password.
- macOS: expansion tooltips reveal truncated field contents.

**Hot Sheet application.** The quick composer and New Ticket sheet default
category/priority/store; git identity and remote URL are read, not asked. Truncated
titles and slugs get expansion tooltips (`title` on web).

### 3.8 Searching

- One clearly identified search that covers everything, with visible scope
  (placeholder, scope bar, or tokens) defaulting to the broad scope; local filters in
  views are fine.
- Search as you type; recent searches before typing; suggestions while typing;
  a way to clear history.
- macOS placement: trailing side of the toolbar for split-view apps; top of the
  sidebar only when it filters the sidebar; focus the field on arrival in a dedicated
  search area.
- Index content in Spotlight; provide Quick Look for custom file types.

**Hot Sheet application.** Workspace header search (trailing) delegates to the
checkout index and already searches slug, title, tags, details, and notes. Add scope
(This project / All projects / Up Next / Closed) and filter tokens (`status:`,
`priority:`, `tag:`, `assignee:`) with suggestions; ⌘F focuses it. Search does not
silently narrow to the active sidebar view. Exact slugs can reveal normally excluded
lifecycle states, while those states otherwise require a visible scope/filter token;
reference-mention results carry a match-reason label. Native: Core Spotlight indexing
of slugs and titles.

### 3.9 Settings

- Good defaults so settings are rarely needed; few settings; ⌘, opens them.
- Never duplicate system settings (appearance, accessibility, scrolling); task
  options (view mode, sort, columns, filters) live in the view, not in Settings.
- macOS settings window: fixed, always-visible pane toolbar that shows the active
  pane; window titled by pane; minimize and zoom dimmed; reopens on the last pane.

**Hot Sheet application.** The current HS1-style category navigator (Ticket sources,
Commands, Permissions, Column view) is compatible: each category is a pane; keep
list/board mode, sort, and column visibility in the workspace. Never add an
appearance toggle. Add ⌘, / Ctrl+, and last-pane restore.

### 3.10 File management

- Avoid explicit save; autosave periodically, on close, and on app switch; show the
  unsaved-changes dot only when autosave is off.
- Use the system open/save panels; support Open Recent; Quick Look for attachments.

**Hot Sheet application.** The 150 ms autosave already satisfies this; show no
Save button or unsaved dot for ticket text and flush on window deactivation and
quit. Attachments preview through Quick Look natively and browser-native previews on
the web; export uses the system Save panel with a format pop-up.

### 3.11 Full screen and multitasking

- Use the system full-screen mode (View › Enter Full Screen, ⌃⌘F); no custom
  window-mode menu; keep essential controls reachable; let people reveal the menu bar
  and Dock; never end full screen automatically.
- Expect to be in the background often; finish user-initiated work there; save and
  restore context; notify only for important completions while away.

**Hot Sheet application.** AI sessions keep running in the background (the server
outlives the client anyway); live updates continue and replay from the cursor on
return; autosave flushes on deactivation. A "focus on ticket" layout that hides
sidebars is a View toggle, not a full-screen substitute.

### 3.12 Notifications

- Ask permission at first relevant use with a reason, never on launch.
- Classify by interruption level: Passive, Active (default), Time Sensitive (now or
  within the hour; breaks through Focus), Critical (never for this app).
- Never send several notifications for one thing; coalesce per subject; no marketing.
- Foreground: no banner, insert the item into the current view and increment the
  in-app count; badges count unread notifications only and clear when read.
- Up to four real actions; none that merely opens the app; error messages are
  alerts, not notifications.

**Hot Sheet application.** An AI tool blocked on a permission decision is the one
Time Sensitive case (with Approve/Deny actions); "worker finished", "FEEDBACK
NEEDED", and "claim expiring" are Active; routine ticket updates never notify. The
Notifications view is the foreground receiver and clears the count. Web: Web
Notifications; Tauri: the notification plugin.

### 3.13 Offering help and onboarding

- No tutorials for standard components; contextual tips (TipKit-style) for features
  of at most three steps, shown once, dismissible, with eligibility and frequency
  rules; help documentation from the Help menu.
- First launch goes straight to work: no wizard, no branding screen, no permission
  or sign-in gate; defaults over setup.

**Hot Sheet application.** First run opens a store picker or an empty project view
with a single "Open Store…"/"Create Store…" action. Tips near Up Next, claim, and
the terminal drawer replace a tutorial and are re-exposed from Help.

### 3.14 Managing accounts and credentials

- No account unless core functionality needs it; delay sign-in; name the method
  ("Sign in with GitHub", "Use API Token"); never prefill or ask for a password when
  another method exists; a discoverable disconnect/forget path that confirms
  completion.

**Hot Sheet application.** Local git stores need no sign-in. Provider connections
follow this pattern in Settings › Ticket sources, with keychain storage and a
"Disconnect and forget credentials" action.

### 3.15 Collaboration and sharing

- Use the system share surface (macOS sharing popover via `ShareLink`) from a
  toolbar Share button or context-menu Share item; short permission summaries;
  a prominent indicator of who is in a shared session.

**Hot Sheet application.** "Copy Link" and "Share…" on tickets; the live claim
lease indicator is the "who is working here" signal. Shared versus private state
stays visually distinct (persisted note vs unsent draft; shared store vs local
overlay).

### 3.16 Charting data

- A chart only when analysis, trend, or comparison is the point; tables for
  providing data. Familiar types, headline summaries, per-mark accessibility labels,
  keyboard traversal, no color-only encodings; small glanceable charts expand into
  larger ones with identical style.

**Hot Sheet application.** Counts and badges are not charts. Future flow/throughput
and cycle-time dashboards ([04](04-core-server-cli.md) §4.3 APIs) use Swift Charts
natively and follow the `dataviz` skill on the web.

### 3.17 Printing

- File › Print (⌘P), dimmed when nothing printable; system print panel.

**Hot Sheet application.** Print the selected ticket or current list; web print
stylesheet hides chrome.

### 3.18 Patterns adopted only by analogy

Playing audio, haptics, video, live-viewing apps, workouts, ratings and reviews are
not desktop-productivity patterns. Rules we keep: system volume governs any sound
cue and cues are off by default and always paired with visuals; the terminal bell
routes to the system alert sound; a trackpad **Alignment** haptic may accompany a
snap (ticket drop slot, splitter hitting its minimum) once, never per pixel; video
attachments use the system player or native `<video>` controls; an active AI session
gets a distinct "live" appearance, an elapsed-time or lease countdown as a local-only
timer, prominent pause/release controls, and a completion summary; never interrupt a
task with a request for anything.

## 4. Components

### 4.1 Windows

- Primary window = navigation plus content; auxiliary window = one task, closes
  itself when done. Open new windows deliberately (offer "Open in New Window"), not
  by default.
- Never draw custom window chrome; use the word "window" in copy.
- macOS states: **main** (frontmost), **key** (accepts input, colored controls),
  **inactive** (gray controls, no materials, subdued). Custom views must follow the
  system appearance for each state.
- Bottom bars hold only small, non-critical status; detail belongs in a trailing
  inspector.

**Hot Sheet application.** Native: `WindowGroup` per project, `Settings` scene,
auxiliary `Window` for a detached terminal or notifications, `.windowResizability`
enforcing the 1024×600 floor. Tauri: native decorations, `minWidth: 1024,
minHeight: 600`, mute accent-colored selection on `blur` so the web UI follows
key/inactive. The project tab bar is a *window* tab bar (Safari/Finder style), so
Window menu tab commands apply (§4.4).

### 4.2 Split views, sidebars, inspectors

- Sidebar (leading) navigates areas or top-level collections; at most **two levels**,
  groups with disclosure, succinct group titles, customizable order, hideable via a
  toolbar button **and** View › Show/Hide Sidebar, never hidden by default, may
  auto-collapse when the window shrinks, nothing critical at its bottom, icons tinted
  by the user's accent color (fixed colors only with a purpose), row size following
  the system sidebar-size setting, content extending beneath it.
- Split view: 1 pt dividers, sane min/max pane sizes so dividers never vanish,
  persistent selection highlighted in every leading pane, drag and drop between
  panes, every pane hideable by toolbar button plus menu command with shortcut.
- Inspector: follows the selection, sits at the trailing side of the split view
  (`.inspector` / `NSSplitViewController`), toggled with ⌥⌘I.

**Hot Sheet application.** Already the shape of AppShell. Keep sidebar regions ≥
250 px, inspector collapsible from the trailing edge, "Add project" near the top
(never a bottom action zone), persisted splitter sizes, and Kerf-owned expansion
state for sidebar groups. Native uses `NavigationSplitView` with `.listStyle(.sidebar)`.

### 4.3 Toolbars

- Three groupings: **leading** (sidebar toggle, back, then the title; not
  customizable), **center** (common controls; customizable; overflows into the
  system-managed menu), **trailing** (always visible: inspector toggle, search, More
  menu, the single prominent primary action).
- At most three groups; fixed space between adjacent text-labeled buttons; borderless
  system symbols without circles; no custom toolbar backgrounds or tints; scroll edge
  effect instead.
- Window title under 15 characters, never the app name; can be inline with controls.
- Every toolbar item is also a menu-bar command (people hide or customize toolbars).
- The system adds the overflow menu; never overflow by default at the minimum width.

**Hot Sheet application.** WorkspaceHeader is the toolbar: leading sidebar toggle +
project title; center list/board segmented switch, sort/group, bulk actions;
trailing search, inspector toggle, "New Ticket" as the one brand-colored action,
More menu. Implement responsive overflow so nothing wraps at 1024 px. Native:
`.toolbar(id:)` items so people can Customize Toolbar…; title = project name.

### 4.4 The menu bar (macOS and Tauri)

Order: **App · File · Edit · Format · View · app-specific · Window · Help**, then
menu bar extras. Rules: support the standard menus and their item order; always show
the same items and **disable rather than hide** unavailable ones; standard shortcuts
for standard items; one-word menu titles; every context-menu, toolbar, Dock-menu,
and sidebar action also appears here; View holds appearance (show/hide toggles whose
titles reflect current state, full screen), Window holds navigation and management
(Minimize, Zoom, tab commands, Bring All to Front, window list), File holds Close.
Dynamic (Option-modified) items are shortcuts, never the only path.

Standard contents that apply to Hot Sheet:

| Menu | Items (in order) |
|---|---|
| Hot Sheet | About Hot Sheet · Settings… ⌘, · Services ▸ · Hide Hot Sheet ⌘H · Hide Others ⌥⌘H · Show All · Quit Hot Sheet ⌘Q |
| File | New Ticket… ⌘N · New Window ⇧⌘N · Open Store… ⌘O · Open Recent ▸ · Close ⌘W / Close Tab · Close Window ⇧⌘W · Export As… · Print… ⌘P |
| Edit | Undo/Redo (result-named) ⌘Z ⇧⌘Z · Cut ⌘X · Copy ⌘C · Paste ⌘V · Paste and Match Style ⌥⇧⌘V · Delete · Select All ⌘A · Deselect All ⇧⌘A · Find ▸ (Find ⌘F focuses search) · Spelling and Grammar ▸ · Substitutions ▸ · Transformations ▸ · Speech ▸ · (Start Dictation, Emoji & Symbols added by the system) |
| View | as List / as Board (checkmarked) · Sort By ▸ · Group By ▸ · Show/Hide Tab Bar · Show/Hide Toolbar · Customize Toolbar… · Show/Hide Sidebar ⌃⌘S · Show/Hide Inspector ⌥⌘I · Show/Hide Terminal · Enter Full Screen ⌃⌘F |
| Project | Switch to open projects · Reload · Ticket Sources… · Commands… · Permissions… |
| Ticket | Open · Open in New Window · Start · Complete · Mark Verified · Report Not Working… · Set Status ▸ · Set Priority ▸ · Set Category ▸ · Move to Up Next / Backlog · Claim / Release · Add Tag… · Duplicate · Copy Slug · Copy Link · Move to Store… · Archive · Delete… |
| Window | Minimize ⌘M · Zoom · Show Previous/Next Tab · Move Tab to New Window · Merge All Windows · Bring All to Front · open windows |
| Help | Hot Sheet Help · Release Notes · Keyboard Shortcuts |

App-specific menus mirror the hierarchy (projects contain tickets), so Project
precedes Ticket. Format is omitted (Markdown, not rich text). Menu bar extra: only if
users enable it in Settings, showing active AI work as a **menu**, never a popover;
the same items must exist elsewhere.

**Hot Sheet application.** Native: `CommandMenu`/`CommandGroup`. Tauri: the same
structure through the Tauri menu API driven by one command registry, which also
feeds the in-browser More menu and the Keyboard Shortcuts reference so no command is
context-menu-only (HS2-80VPPW, with HS2-KTHGVE for the shortcut table).

### 4.5 Menus, context menus, pop-up and pull-down buttons

Label grammar for **all** menus: verb-first title case, no articles, an ellipsis (…)
when more input follows, show/hide labels that flip with state, checkmarks for
attributes in effect, separators for at most about three logical groups, one submenu
level (a submenu when a term repeats in more than two items: "Set Status ▸"), icons
applied to all items in a group or none.

Context menus: relevant and short; identical on every surface that has one; **hide**
inapplicable items (persistent menus disable instead) except Cut/Copy/Paste, which
may dim; most-used items nearest the pointer; destructive items last; no keyboard
shortcut hints; every item also in the menu bar; Control-click and secondary click
open it; a title only for multi-selection ("4 Tickets").

Pop-up button (`Picker(.menu)` / `wa-select`): mutually exclusive states with a
default and a label that predicts the options; the closed control shows the selected
label **and** its icon/color. Pull-down button (`Menu` / `wa-dropdown` + `wa-menu`):
at least three related actions; never a place to hide a view's primary action;
destructive items confirm.

**Hot Sheet application.** The TicketRow context menu (list and board) already
carries Lucide icons on every item (project rule satisfies "all or none"); keep it
identical through TicketList and TicketBoard, hide inapplicable items (Release when
unclaimed), keep Delete… last, and mirror every item into the Ticket menu. Status,
priority, and category selects render the selected option's icon in the closed
state (the "complete selected presentation" test rule).

### 4.6 Lists, tables, outline views, collections

- Tables for text; collections for image-heavy items. Column headings: title-case
  nouns, no punctuation. Click a heading to sort; click again to reverse. Resizable
  columns; alternating row backgrounds for wide multi-column tables; persistent
  selection for navigation tables; middle ellipsis for identifiers.
- Outline views for hierarchy: disclosure triangles in the first column, sorting
  applied per level, Option-click expands all, expansion state persisted across
  sessions.
- Selection colors: accent highlight with white text when the list has focus, gray
  highlight when it does not.

**Hot Sheet application.** TicketList is a table (`Table` natively; Kerf list with
`role="grid"`, `aria-sort`, resizable headers on the web); the board is a collection
of column lists using the same responsive ticket-summary component. The project
sidebar is an outline (`wa-tree`/`OutlineGroup`) with persisted expansion. Selected
rows switch between accent and gray by list focus (web today uses one blue state;
HS2-A2A9GT).

### 4.7 Presentation: which surface to use

| Need | Use | Rules |
|---|---|---|
| A few related, safe-to-dismiss options anchored to a control | **Popover** (`.popover` / `wa-popover`) | One at a time; never a popover on a popover; never for warnings; autosave on light dismiss, discard only on explicit Cancel; detachable into a panel on macOS |
| A scoped task before returning to the parent | **Sheet** (`.sheet` / modal `wa-dialog`) | One per window; Cancel plus a verb-named default (never Done alone); Esc cancels; other windows stay usable; if people must watch results while iterating, use a panel or inspector instead |
| Critical, unexpected, non-undoable information or confirmation | **Alert** (`.alert` / `NSAlert` / alert-style `wa-dialog`) | Title says what and why (never "Error"); ≤ 3 buttons; specific verbs, "OK" only when purely informational, never Yes/No; default trailing, Cancel leading and never default; destructive style only when the person did not choose the action; caution icon rarely; Esc and ⌘. cancel; suppression checkbox for repeats |
| Choices related to an action just taken | **Confirmation dialog** (`confirmationDialog` / small `wa-dialog`) | Destructive first and styled, Cancel last; short title; no scrolling list of choices |
| Selection-following details or tools used while watching results | **Inspector pane or panel** (`.inspector`, `NSPanel` / `wa-drawer`) | Noun title; follows app activation; not in the Window menu's document list |
| Self-contained work | **New window** | Offer via context menu / File menu |

Never use `window.alert`/`confirm` on the web; disable `light-dismiss` on
data-bearing dialogs. The permission-request popup is an expected, actionable
request, not an alert; it is a non-modal popup that queues (§3.4).

### 4.8 Buttons

- Style + content + role. One prominent (accent-filled) action per view, wired to
  Return; Cancel wired to Escape; destructive buttons red and **never** default.
- Verb-first title-case labels; trailing ellipsis when the button opens another
  window or view; tooltip on every button; ≥ 44 pt hit region with spacing; a press
  state on custom buttons.
- macOS: square/gradient buttons (symbols only) beside tables for add/remove; help
  button (one per window, lower corner opposite the dismiss buttons); image buttons
  with ~10 px padding and no border; spring loading on drags.

**Hot Sheet application.** `wa-button variant="brand"` is the one primary per
surface; `variant="danger"` never receives autofocus; icon-only buttons carry
`wa-tooltip`/`title` and an accessible name; "Bulk Edit…", "Move to…",
"Report Not Working…" end with an ellipsis.

### 4.9 Text fields, text views, token fields, combo boxes

- Text field for a small specific value; text view for multi-line content; secure
  field for secrets; placeholder **plus** a persistent label; width matched to
  expected content; consistent widths in a form; logical tab order; validation timed
  to the field (on focus loss for formats, before it for uniqueness); number
  formatters; expansion tooltips for clipped text; useful text always selectable.
- Token fields for lists of discrete values with suggestions, Return/comma
  tokenization, a context menu per token, and a tuned suggestion delay. Combo box
  when free text plus known values.

**Hot Sheet application.** Title is a field; details, notes, and blocked reason are
text views with the 150 ms autosave and no Save/Cancel (project rule). Tags,
assignees, related tickets, and search filters are token inputs (`wa-input` +
`wa-tag` chips + suggestion popup). Provider tokens use secure fields.

### 4.10 Toggles, pickers, segmented controls, sliders, steppers

- Toggles only in the window body (never in a toolbar; toolbars use pressed-state
  icon buttons). Checkboxes for hierarchies and mixed state; switches for emphasized
  or section-level settings (mini switches in grouped forms); radio buttons for 2–5
  exclusive options; a pop-up beyond five. State never by color alone.
- Segmented control: 2–7 equal segments of one content type, never mixing actions
  with selection; tooltips on icon segments; in the toolbar or inspector, not as the
  main-area view switcher (that is a tab view).
- Pickers: short lists → pop-up; long → picker or table; dates → textual date picker
  on macOS; present in place, never by navigating away.
- Sliders with live feedback, tick marks, hover value tooltip; steppers beside a
  visible value, Shift-click for 10× steps.

**Hot Sheet application.** List/Board is a segmented control in the header;
inspector sub-panes are a tab view; Up Next is a checkbox in the inspector and a
pressed-state icon button in the toolbar; settings forms use grouped checkboxes with
indentation and mixed-state parents for notification categories.

### 4.11 Search fields, tab views, disclosure, boxes, labels, scroll views

- Search field: search icon, clear button, placeholder naming the scope; trailing
  toolbar placement (§3.8).
- Tab view: ≤ 6 title-case noun tabs; never switch tabs with a pop-up; inset from
  window edges. Tab bars (navigation) never disable or hide a tab; badges only for
  critical information.
- Disclosure triangle points to the trailing side when collapsed and down when
  expanded; disclosure button (chevron) at most one per view; persist expansion.
- Boxes group related controls with a sentence-case title above (colon-suffixed in
  settings panes); no nested boxes.
- Labels use the four label colors for hierarchy and are selectable when useful
  (ids, paths, errors).
- Scroll views own one axis each (never nest same-axis), keep native gestures and
  keyboard scrolling, auto-scroll only the minimum to reveal a selection or find hit,
  use the automatic scroll edge effect beneath floating bars; macOS scroll bars follow
  the system overlay setting.

**Hot Sheet application.** ProjectTabBar tabs are never disabled; a tab badge shows
only pending permission counts (already the behavior). Inspector tabs ≤ 6. Sidebar,
workspace, inspector, and terminal each own exactly one scroll region; terminal
output auto-scrolls only while pinned to the end.

### 4.12 Progress, gauges, notifications, widgets, App Intents

- Progress: transient; determinate when possible; accurate and even; keep moving;
  never morph spinner↔bar; consistent placement; Cancel (and Pause when work would
  be lost). macOS: unlabeled spinners for background work and tight spaces; bars for
  known extents with specific text.
- Gauges/level indicators for a value in a range with labeled endpoints; tiered color
  near limits plus a non-color cue.
- Notifications: see §3.12. Widgets (Mac desktop and Notification Center, small to
  extra large, full-color and vibrant modes, ≥ 11 pt text, smaller Mac margins):
  dynamic, deep-linked, glanceable, never a launcher, never mirrored inside the app.
  App Intents (not App Shortcuts, which macOS lacks) expose ticket actions to
  Shortcuts and Spotlight; snippets ≤ 400 pt with verb-named confirmations.

**Hot Sheet application.** The busy spinner stays unlabeled and in one place; sync
and bulk operations get bars with "Moving 4 of 12 tickets…" text; the claim-lease
countdown is a local-only gauge. Native roadmap: an Up Next widget and App Intents
(`CreateTicket`, `StartTicket`, `CompleteTicket`, `ClaimNextTicket`,
`OpenTicket(slug)`) mirroring CLI verbs (§8.1).

### 4.13 Standard icons for actions

Use the same metaphor everywhere an action appears (menu bar, context menu,
toolbar, buttons). Lucide names for the web; SF Symbol names for the native app if
HS2-0P83KD chooses SF Symbols.

| Action | Lucide | SF Symbol |
|---|---|---|
| Cut / Copy / Paste | `scissors` / `copy` / `clipboard-paste` | `scissors` / `document.on.document` / `document.on.clipboard` |
| Done / Cancel | `check` / `x` | `checkmark` / `xmark` |
| Delete | `trash-2` | `trash` |
| Undo / Redo | `undo-2` / `redo-2` | `arrow.uturn.backward` / `arrow.uturn.forward` |
| New / Compose | `square-pen` | `square.and.pencil` |
| Duplicate / Rename / Move to | `copy-plus` / `pencil` / `folder` | `plus.square.on.square` / `pencil` / `folder` |
| Attach / Add / More | `paperclip` / `plus` / `ellipsis` | `paperclip` / `plus` / `ellipsis` |
| Search / Filter | `search` / `list-filter` | `magnifyingglass` / `line.3.horizontal.decrease` |
| Share / Print | `share` / `printer` | `square.and.arrow.up` / `printer` |
| Archive / Calendar / Account | `archive` / `calendar` / `circle-user` | `archivebox` / `calendar` / `person.crop.circle` |
| Sidebar / Inspector toggle | `panel-left` / `panel-right` | `sidebar.leading` / `sidebar.trailing` |

### 4.14 Components not adopted

Column views (`NSBrowser`) — Hot Sheet's hierarchy is shallow and needs sorting; a
sidebar + table + inspector is right. Page controls, ornaments, lockups, home screen
quick actions, digit entry, virtual keyboards, activity rings, complications,
controls, Live Activities, status bars, Top Shelf, watch faces — other platforms.
Rating indicators — priority is a named picker, not stars. The transferable rules
(consistent badge corners, ≥ 2 pt strokes on tiny glyphs, "feature what is new,
never advertise") are folded into §2 and §3.

---

## 5. Inputs

### 5.1 Keyboards

- Full Keyboard Access everywhere; never repurpose a standard shortcut (the only
  sanctioned exception: a standard action that cannot apply, e.g. ⌘I when there is
  no italic).
- Custom shortcuts only for the most frequent app-specific commands. Command is the
  main modifier; Shift complements a related shortcut; Option sparingly; **avoid
  Control**. List modifiers in the order Control, Option, Shift, Command. Do not add
  Shift for an upper character (⌘? not ⇧⌘/). Do not build a new shortcut by adding a
  modifier to an unrelated existing one.
- Let the system localize and mirror shortcuts.

**Hot Sheet shortcut register** (macOS keys; the web client maps ⌘→Ctrl on Linux and
Windows and must not intercept Control combinations on macOS, which belong to the
terminal and the system):

| Shortcut | Command | Notes |
|---|---|---|
| ⌘N | New Ticket… | |
| ⇧⌘N | New Window | |
| ⌘O / ⌘W / ⇧⌘W / ⌘Q / ⌘M / ⌘H | Open Store… / Close (Tab) / Close Window / Quit / Minimize / Hide | system meanings, never rebound |
| ⌘, | Settings… | |
| ⌘Z / ⇧⌘Z | Undo / Redo (result-named) | text-level undo inside editors |
| ⌘X / ⌘C / ⌘V | Cut / Copy / Paste ticket(s) when the work area owns focus; text otherwise | existing focus-ownership rule |
| ⌘A / ⇧⌘A | Select All / Deselect All tickets in the focused list | |
| ⌘F / ⌥⌘F | Focus workspace search | |
| ⌘J | Scroll to selected ticket | |
| ⌃⌘S / ⌥⌘I | Show/Hide Sidebar / Inspector | |
| ⌃⌘F | Enter/Exit Full Screen | system |
| ⌘P | Print… | |
| ⌘? | Help | |
| ⇧⌘[ / ⇧⌘] | Previous / Next project tab | window-tab convention |
| ⌘1 / ⌘2 | View as List / as Board | app-specific, Command-only |
| Return / ⌘↓ | Open selected ticket in the reader | `Return` opens; `⌘↓` mirrors Finder |
| Space | Quick Look-style preview of an attachment | when the attachment list has focus |
| Delete / ⌘Delete | Archive selected / Delete… selected | Delete-key parity with Edit › Delete |
| Esc / ⌘. | Cancel, close popover or dialog, clear search | |

The single source for this table is the command registry (HS2-80VPPW, HS2-KTHGVE).

### 5.2 Pointing devices

- Consistent behavior for mouse and trackpad; never redefine systemwide gestures;
  modifier behavior identical across input devices (Option-drag copies).
- Hit regions padded ~12 pt (bezeled) / ~24 pt (bezel-less), contiguous across
  adjacent toolbar buttons; no gratuitous pointer effects; never scale a hovered table
  row (tint only).
- Cursors tell the truth: arrow for content and standard controls on macOS; I-beam for
  editable text; open/closed hand for repositioning; resize cursors on splitters;
  drag-copy under Option; operation-not-allowed over invalid drops; pointing hand for
  URL-style links; contextual-menu cursor while Control is held.

**Hot Sheet application.** The project's cursor rule (pointer for clickable
controls and selectable rows, text for editable text, not-allowed for disabled,
grab/resize for direct manipulation, default otherwise) is the web translation and is
kept deliberately even though AppKit shows an arrow over buttons. `cursor-semantics.css`
already covers native and Web Awesome controls; extend `dropEffect` to `copy` under
Option/Alt and `none` over invalid targets. Splitter handles keep a ~12 px hit area
around a 1 px line.

### 5.3 Focus and selection

- Rely on system focus effects. Focus rings on text and search fields; whole-row
  highlight in lists (accent with white text when the list has focus; gray when it
  does not).
- Never move focus without the person's action. If the focused item disappears, move
  to a neighbor only when the person was arrow-navigating; otherwise hide the focus
  indicator.
- On macOS, Full Keyboard Access reaches controls; the app supplies focus for content
  elements (rows, fields).

**Hot Sheet application.** Live updates (long poll/WebSocket morphs) must never
steal or move focus — already a render-budget test rule; when an AI worker moves or
deletes the selected ticket, the selection empties and focus is hidden rather than
jumping. Roving `tabindex` inside lists (arrows within, Tab between regions). The
terminal drawer shows a visible focus indication on its container so keystrokes
visibly go to the PTY. Hover-revealed row actions stay reachable by keyboard or the
context menu.

### 5.4 Gestures

- macOS is keyboard-and-pointer first; trackpad gestures (swipe between pages, pinch,
  rotate, smart zoom, force click) are secondary and customizable. Touch-and-hold is
  not a Mac gesture.
- Never make a gesture the only path; give immediate feedback; show when a gesture is
  unavailable (a locked object that does not move reads as a freeze).

**Hot Sheet application.** Drag and drop is a convenience over "Move to…", "Set
Status", and shortcuts. A ticket that cannot be dragged (live claim by another
worker, provider without the capability) looks visibly different and shows
`not-allowed`. Do not hijack horizontal two-finger swipes unless a real page stack
exists.

### 5.5 Inputs not adopted

Action button, Apple Pencil and Scribble, Camera Control, Digital Crown, eyes, game
controls, gyroscope, nearby interactions, remotes. Kept principles: verb-first
≤ 3-word action labels ("Claim Next Ticket"), 44 pt / 28 pt minimum targets with a
press state, user-remappable shortcuts if ever offered, and never revealing content
next to the element someone is about to activate.

---

## 6. Technologies

### 6.1 Generative AI and machine learning (adopt fully)

Hot Sheet's purpose is surfacing AI work, so these two pages carry as much weight as
any component page.

- **Disclose.** Clearly identify when and where AI is used; never let AI output pass
  as human-authored. State capabilities and limits up front; say that output may
  contain errors where narration summarizes state.
- **Keep people in control.** Honor in-scope requests; let people dismiss, revert,
  retry, or edit generated content; put Edit / Undo / Retry / Adjust next to results
  and acknowledge when a correction takes effect.
- **Confirm consequences.** Ask before a significant action on someone's behalf;
  never automate destructive or hard-to-undo actions on model output alone.
- **Scope hallucination risk.** Narrow requests; avoid factual claims the model cannot
  verify; avoid AI content where a mistake could harm.
- **Latency and progress.** Generate in the background; give specific progress text
  ("Running tests for HS2-…", not "Processing…"); on failure, plain-language cause
  and next step.
- **Fallback and opt-out.** The product works with AI features off; offer a non-AI
  path.
- **Privacy.** Prefer local processing; minimize what leaves the machine; disclose
  which tool or server receives content and whether it may be stored or used for
  training; ask before using personal or usage data; offer opt-out.
- **Feedback loop.** Voluntary, unobtrusive feedback (thumbs up/down plus optional
  detail) using consequence language ("Stop narrating test runs"); act immediately
  and persist; rejected suggestions do not reappear.
- **Machine learning patterns.** Attribution is factual ("Because the ticket has no
  category"), never emotional; confidence shown through ranking, thresholds, or
  semantic categories rather than percentages; proactive suggestions need a higher
  bar and a confidence threshold; corrections are guided (accept, choose an
  alternative, edit) and persist; limitations are explained in context; test
  adversarially with vague, sensitive, and out-of-scope prompts.

**Hot Sheet application** (HS2-WBW3Z9): AI notes, distilled activity notes, ticket
repair proposals, narration, and tool-generated fields carry a persistent AI label
that is part of the row/note accessible name; AI-driven mutations show Undo/Retry/Edit
and confirm before irreversible steps; repair is a guided correction that never
auto-applies below threshold; progress text names the agent's current step; settings
disclose which tool receives ticket text; feedback is stored as a regular note. The
durable AI note policy is HS2-3GRNZW; permission prompts remain the human-in-the-loop
control ([05](05-ai-tool-plugins.md) §5.7).

### 6.2 VoiceOver

- Descriptive labels on every key element; decorative images hidden; images described
  only for what they add; unique titles and headings; grouping and reading order
  declared; layout changes announced; rotor support.

**Hot Sheet application.** A ticket row's accessible name is title + status +
priority (+ Needs review, blocked, live claim, AI-authored where applicable).
Activity and agent events post polite live-region announcements, never every PTY
line. Inspector sections are real headings.

### 6.3 Mac Catalyst (by analogy)

Not used, but its "make it feel like a Mac app" list is the cross-client checklist:
sidebar split view instead of tabs, every command in the menu bar, toolbar commands
at the top, inspector beside content (not a popover), a contextual menu on every
object, Next/Previous controls in addition to gestures, top-down flow, no
reachability-driven bottom bars, text styles instead of fixed sizes, 100% scale.

### 6.4 Siri, App Intents, iCloud, Sign in with Apple, SharePlay

- App Intents (native, later): `CreateTicket`, `StartTicket`, `CompleteTicket(note)`,
  `ClaimNextTicket`, `OpenTicket(slug)`; a `Ticket` entity donated to Spotlight;
  responses succinct and specific ("Started HS2-7F3K9Q", "That ticket is claimed by
  another worker"), no app name, no impersonation of Siri.
- iCloud rules by analogy for git/server sync: silent automatic sync, an unobtrusive
  offline note, early side-by-side conflict resolution, deletion warnings that
  mention collaborators and AI workers, search that includes not-yet-indexed items or
  says so.
- Sign in with Apple only if a hosted account ever exists; provider connections
  already follow its rules (delay, explain, name the method, show who is signed in).
- SharePlay principles for collaboration: shared vs private state visibly distinct;
  a simple explicit concurrency rule surfaced in the UI; late joiners brought up to
  date by replay without disrupting others; easy detach and reattach.

### 6.5 Technologies not adopted (principles kept)

AirPlay, Always On, App Clips, Apple Pay, augmented reality, CareKit, CarPlay, Game
Center, HealthKit, HomeKit, ID Verifier, iMessage apps, in-app purchase, Live Photos,
Maps, NFC, photo editing, ResearchKit, ShazamKit, Tap to Pay, Wallet. Rules worth
keeping: never hide or disable a primary action — explain the blocker after the
click; validation copy is a specific noun phrase in sentence case, ≤ 128 characters,
no period; defer to the authoritative external system and show conflicts side by side
(GitHub Issues and Jira are authoritative, per [16](16-external-sync-interface.md));
never write to shared data without explicit direction (the constraint on AI repair);
notify only for time-critical changes; keep badges in one consistent corner; exact
product terminology, never renamed system concepts; land on the most relevant screen
and never re-ask for sign-in.

## 7. Where Hot Sheet 2 stands today

An honest reading of the shipped web client against §2–§6 (2026-09-03). Items in
the "gap" column are tracked by the tickets in §11.

| Area | Already conforms | Gap |
|---|---|---|
| Structure | Sidebar · workspace · inspector split view with persistent splitters, 1024×600 floor, inspector-first collapse, header-owned restore controls, terminal drawer | Key/inactive window muting (Tauri), background extension under chrome, scroll-edge treatment |
| Menus and commands | Icon-bearing context menus identical in list and board; outside-click and Esc dismissal | No menu bar / command registry; no in-browser command surface; shortcut table not centralized (HS2-80VPPW, HS2-KTHGVE) |
| Editing | 150 ms autosave, no Save/Cancel, field-aware live merge, reader escalation | Result-named undo entries; Edit-menu wiring |
| Selection and focus | Native click/⌘/⇧ selection model; no focus theft on live updates; work-area focus ownership | Accent-vs-gray highlight by list focus; roving tabindex audit (HS2-A2A9GT) |
| Feedback and modality | Non-modal permission popups with queueing; inline connection banner; silent background reconciliation | Dialog button grammar audit; specific progress text; ellipsis rule (HS2-A2A9GT) |
| Color and appearance | Semantic Web Awesome tokens, `--hs-*` aliases limited to domain states, literal-token lint | Light-only; no `prefers-color-scheme`, `prefers-contrast`, `prefers-reduced-motion`, `prefers-reduced-transparency` (HS2-DKZG9S) |
| Typography | Token-based sizes | Inter-first stack instead of system font; scale not mapped to macOS text styles (HS2-PS29TA) |
| Icons | Lucide only through a shared component with the icon-policy test | Native decision pending (HS2-0P83KD); standard-action map in §4.13 to enforce |
| Cursors | `cursor-semantics.css` for native and Web Awesome parts, splitter cursors | `dropEffect` copy/none semantics |
| Notifications | Permission events, history view, "Decision made outside Hot Sheet" | Interruption-level policy and coalescing once native/Tauri notifications ship |
| AI content | Attributed activity timeline; Attempt AI repair creates a ticket instead of editing files | Persistent AI labels, feedback, undo/retry affordances (HS2-WBW3Z9) |
| Help | Tooltips on icon actions | 60–75 character verb-first audit; contextual tips; Help menu content |

---

## 8. Leveraging the HIG as the clients grow

### 8.1 The native SwiftUI macOS app (client step 3)

Build order and the HIG surfaces each step unlocks:

1. **Scenes and windows.** `WindowGroup` per project window with `NavigationSplitView`
   (sidebar · content · detail) plus `.inspector`, native window tabbing for project
   tabs, `.defaultSize` and the 1024×600 minimum, `Settings` scene with pane
   toolbar, state restoration. Auxiliary `Window` scenes for a detached terminal and
   the Notifications history.
2. **Menu bar and commands.** `CommandMenu`/`CommandGroup` implementing §4.4 exactly,
   sharing titles, order, enablement, and shortcuts with the command registry the web
   client already uses. Disable, never hide. Dock menu with New Ticket… and recent
   projects.
3. **Content views.** `Table` for the ticket list (sortable, resizable, alternating
   rows, multi-selection, drag), a `LazyVGrid`/`LazyHStack` board using the same
   ticket-summary contract, `OutlineGroup` sidebar with persisted expansion, a
   `TextEditor`-based details/notes editor with the same autosave and field-aware
   merge semantics, `.searchable` with scopes and tokens.
4. **Presentation.** `.sheet` for New Ticket, provider connection, bulk operations,
   Not Working evidence; `.alert`/`NSAlert` only for irreversible surprises; popovers
   (detachable) for filters and quick pickers; `confirmationDialog` for post-action
   choices.
5. **System integration.** `UNUserNotificationCenter` with interruption levels and
   Approve/Deny actions; App Intents for CLI-parity verbs plus a `Ticket` entity in
   Spotlight; a WidgetKit Up Next widget (small/medium/large, vibrant-safe);
   `ShareLink` for ticket links; Quick Look for attachments; optional
   `MenuBarExtra` (user-enabled, menu not popover) showing active AI work; Services
   and Open Recent for stores.
6. **Terminal.** A native terminal surface in a bottom split (content layer, no
   glass) honoring the server-arbitrated sizing model ([06](06-clients.md) §6.7),
   with a visible focus state and the standard find-in-scrollback.
7. **Polish gates.** Icon Composer app icon; Developer ID signing and notarization;
   VoiceOver, Full Keyboard Access, Increase Contrast, Reduce Transparency, Reduce
   Motion passes; light and dark; graphite accent; the visual QA rule from CLAUDE.md.

Decision needed before step 3: SF Symbols vs Lucide in the native app (HS2-0P83KD).

### 8.2 The Tauri host (client step 2)

- Native window decorations and minimum size; never a CSS title bar.
- A native menu built from the command registry (§4.4), with platform-appropriate
  modifiers and the same enablement rules; on Linux/Windows the menu renders
  in-window with Ctrl/Alt conventions.
- Notification plugin for background events; a tray item may show active AI work
  (compact status, updated only on change, cleared when the claim ends).
- Key/inactive window state mirrored into the web UI (`blur`/`focus` events → muted
  selection accent).
- OS-level file drops for attachments; "Reveal in Finder" replaces browser download
  where the platform allows.
- Bundled Web Awesome assets, no external requests, production Vite output only
  (see [06](06-clients.md) §6.3 startup budget).

### 8.3 The browser client (now)

Everything in §2–§6 marked Adopt or Translate, delivered through Web Awesome tokens
and parts, Kerf state, and the `/ux-demo` catalog. The catalog must show every
supported variant and state (project rule), which is how the HIG's "consistent
presentation across surfaces" is proven for each component.

### 8.4 iOS, Android, and beyond

iOS (client step 4) reuses this document plus the iOS platform considerations in the
digests (HS2-46RA38): tab bar or adaptable sidebar, sheets with detents, edit menus,
44 pt targets, Dynamic Type, Live Activities for active claims, App Shortcuts, Home
Screen quick actions, a widget family, and the remote-first permission-answering role
from [08](08-distributed-and-remote.md). Android (step 5) keeps the behavioral rules
(menus, undo, feedback, modality, focus, AI disclosure) and adopts Material
conventions for presentation. Any future surface — a menu bar extra, a Shortcuts
action, a widget, a Spotlight result — is designed from the relevant HIG page first
and recorded here.

---

## 9. Hot Sheet rules that override or extend the HIG

These are deliberate. Do not "fix" them toward the HIG without a maintainer decision.

| Hot Sheet rule | Relationship to the HIG |
|---|---|
| **Lucide icons only** in the web client and as the shared metaphor vocabulary (CLAUDE.md, [09](09-technology-decisions.md) §9.5) | The HIG assumes SF Symbols on Apple platforms. Keep Lucide on the web; native decision pending (HS2-0P83KD). Standard-action metaphors still align (§4.13). |
| **Every actionable context-menu item carries an icon** | The HIG says icons per group all-or-none and sparingly; Hot Sheet chooses "all", which satisfies uniformity. |
| **Autosave at 150 ms with no Save/Cancel buttons** for details, notes, titles, tags, blocked reasons | Consistent with "avoid explicit save"; Hot Sheet fixes the debounce and forbids routine Save/Cancel. Explicit submission remains for creating objects and completing workflows. |
| **Cursor semantics: `pointer` on clickable controls and selectable rows** | The HIG/AppKit shows an arrow over buttons and rows; the web convention is kept deliberately (§5.2). |
| **1024×600 AppShell floor**; sidebars ≥ 250 px; no auto-hiding at breakpoints | A Hot Sheet-specific application of the HIG's minimum-size and "defer compact layouts" guidance. |
| **No fixed-interval polling; WebSocket or long polling only** | Not a HIG topic; it underpins the "silent, automatic sync" and "never steal focus" behaviors. |
| **Feedback needed = Needs review** with a purple rail that outranks blocked and Up Next rails | Hot Sheet's own state model; obeys "never color alone" via the badge text. |
| **Permission popups are non-modal and visible across projects; auto-Allow/Deny timers are opt-in per project** | The HIG discourages time-boxed UI; Hot Sheet's timers are explicit user settings, paused when hidden, with a visible pause control — an accepted, documented deviation. |
| **Provider capability gating**: unsupported actions are hidden or explained, never emulated | Matches "show when a gesture/command is unavailable"; extends it to provider capabilities. |
| **Visual QA in a real browser before completion** | Stricter than the HIG's "preview on devices"; it is the enforcement mechanism for everything in this document. |

---

## 10. Checklists for AI tools

### 10.1 Before building or changing a surface

- [ ] Which client(s)? Native → adopt the system component; web → the Web Awesome
      primitive styled through tokens/parts; custom only for Hot Sheet-specific
      content.
- [ ] Which presentation (§4.7)? Popover, sheet, alert, confirmation dialog,
      inspector/panel, or window — and is it one modal layer at a time?
- [ ] Where does the command live? Menu bar entry (title grammar, enablement,
      shortcut), context menu, toolbar group, and the in-browser command surface.
- [ ] What is the undo story? Result-named entry, reveal-on-undo, provider capability.
- [ ] What is the feedback story? Passive inline status; alert only for irreversible
      surprises; specific progress text; failure reason and next step.
- [ ] What is the empty state, the error state, the disconnected state, the
      unsupported-capability state?

### 10.2 Per component

- [ ] Labels: verb-first title case for menu items/buttons/tabs/headings; sentence
      case for body, tooltips, placeholders, settings labels; ellipsis when more input
      follows; show/hide titles reflect state.
- [ ] One prominent (accent) action per surface; Cancel never default; destructive
      never default; Esc and ⌘. cancel.
- [ ] Every icon-only control: accessible name + 60–75 character verb-first tooltip;
      decorative icons hidden from assistive technology.
- [ ] Hit targets ≥ 28 px (min 20), ~12 px padding; contiguous toolbar buttons.
- [ ] Color: semantic tokens only; light + dark + increased-contrast values; never
      color alone; one accent.
- [ ] Type: system stack, 13 px body, 10 px floor, no light weights, multi-line before
      truncation, middle-ellipsis for identifiers.
- [ ] Focus: visible ring on fields; accent/gray row highlight by list focus; never
      moved by the app.
- [ ] Motion: ≤ 250 ms, non-bouncy, interruptible, fades under Reduce Motion.
- [ ] Cursor per §5.2; drag feedback per §3.6; keyboard/menu equivalent for every drag.
- [ ] Text always selectable when useful; secure fields for secrets; validation next to
      the field.
- [ ] Scroll: one axis per region; auto-scroll minimally; scroll-edge treatment under
      floating chrome.
- [ ] AI content (§6.1): labeled, undoable/retryable, confirmed before irreversible
      steps, specific progress, feedback affordance.

### 10.3 Before completing the ticket

- [ ] `/ux-demo` shows every supported variant and state; browser tests exercise each
      child action through every shipped composition.
- [ ] Bidirectional state tests (control → state, state → live control) for stateful
      controls; complete selected presentation asserted after transitions.
- [ ] Visual QA in a real browser at a wide size and at 1024×600, light and dark once
      HS2-DKZG9S lands, with Reduce Motion and Increase Contrast when the change
      touches motion or color; states and viewports recorded on the ticket.
- [ ] Docs updated in the same change: [ux-components.md](ux-components.md),
      [06-clients.md](06-clients.md), [TEST-COVERAGE.md](TEST-COVERAGE.md), and this
      document when a rule changes.

---

## 11. Follow-up tickets created with this document

| Ticket | Scope |
|---|---|
| HS2-DKZG9S | Web client follows system appearance: dark palette for every token and `--hs-*` alias, `prefers-color-scheme`, `prefers-contrast`, `prefers-reduced-motion`, `prefers-reduced-transparency`, contrast verification |
| HS2-PS29TA | Web typography: system font stack decision and a 13 px HIG-aligned type scale |
| HS2-0P83KD | Decision: SF Symbols vs Lucide in the native macOS app (with a metaphor map) |
| HS2-80VPPW | Command registry driving the Tauri native menu, the in-browser command surface, and the SwiftUI menu bar; coordinates with HS2-KTHGVE (platform-aware shortcuts) |
| HS2-A2A9GT | HIG conformance audit of every shipped web surface against §10 |
| HS2-WBW3Z9 | AI labeling, feedback, undo/retry, and confirmation affordances per §6.1 |
| HS2-46RA38 | iOS/iPadOS platform considerations before the iOS client starts |

---

## 12. Sources

Apple Human Interface Guidelines, macOS Tahoe edition (Liquid Glass design system;
page change logs through June 8, 2026), read in full on 2026-09-03:

- [Designing for macOS](https://developer.apple.com/design/human-interface-guidelines/designing-for-macos)
- [Foundations](https://developer.apple.com/design/human-interface-guidelines/foundations)
  (18 pages: accessibility, app icons, branding, color, dark mode, icons, images,
  immersive experiences, inclusion, layout, materials, motion, privacy, right to left,
  SF Symbols, spatial layout, typography, writing)
- [Patterns](https://developer.apple.com/design/human-interface-guidelines/patterns)
  (25 pages)
- [Components](https://developer.apple.com/design/human-interface-guidelines/components)
  (72 pages across content, layout and organization, menus and actions, navigation
  and search, presentation, selection and input, status, system experiences)
- [Inputs](https://developer.apple.com/design/human-interface-guidelines/inputs)
  (13 pages)
- [Technologies](https://developer.apple.com/design/human-interface-guidelines/technologies)
  (29 pages)

Individual pages live under
`https://developer.apple.com/design/human-interface-guidelines/<page-slug>`; cite the
slug in a ticket when a rule here needs its source.
