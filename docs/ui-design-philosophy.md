# Kerf UI design philosophy

> **Status: Decided** (HS2-DSXDBP). This is the portable design philosophy for
> Hot Sheet's Kerf UI work and the starting point for the standalone Kerf UI
> project. It synthesizes the Apple HIG guidance in
> [design-guidelines.md](design-guidelines.md) with the recurring lessons from Hot
> Sheet 2 UI tickets. It defines what a good result feels like and how to resolve
> design tradeoffs; it does not replace product requirements or component contracts.

Related docs: [00-vision-and-principles.md](00-vision-and-principles.md),
[06-clients.md](06-clients.md), [design-guidelines.md](design-guidelines.md),
[ux-components.md](ux-components.md), and
[18-dev-review-tool.md](18-dev-review-tool.md).

---

## 1. The promise

Kerf interfaces should feel **calm, legible, stable, and honest**.

They should preserve the user's train of thought while data, automation, and AI work
continue around them. The interface should explain its structure without excessive
chrome, behave like the platform without imitating it superficially, and use a small
semantic vocabulary consistently enough that users learn it once.

The quality bar is not merely that every control exists or that a screenshot resembles
a wireframe. A finished interface must remain coherent while it is used: controls work,
state transitions preserve context, content survives background updates, layouts hold
at supported sizes, and the implementation shown in a demo is the implementation users
receive.

In short:

> Protect continuity, make meaning visible, and spend visual emphasis only where it
> helps the user decide or act.

## 2. How decisions are made

When concerns compete, use this order:

1. **Explicit product intent and user direction.** Requirements and deliberate
   wireframe relationships outrank general convention.
2. **User continuity, data safety, and truthful behavior.** Do not lose work, hide
   state, or imply that an unavailable action works.
3. **Accessibility and platform conventions.** Keyboard, focus, assistive technology,
   contrast, language direction, and host-platform behavior are core behavior.
4. **Shared semantic systems and component contracts.** Prefer learned, predictable
   meaning over local novelty.
5. **Local visual polish.** Refine rhythm, optical balance, and expression after the
   structure and behavior are sound.

A wireframe specifies hierarchy, relationships, and intended feel. It is not a demand
for brittle pixel copying. Responsive adaptations may change presentation, but must
preserve the wireframe's reading order, grouping, emphasis, and available actions.

The supporting documents have distinct jobs:

| Source | What it governs |
|---|---|
| Product requirements and explicit direction | What the product must do |
| This philosophy | How to judge tradeoffs and overall UI quality |
| [Design guidelines](design-guidelines.md) | Concrete Apple HIG and platform rules |
| [UX component catalog](ux-components.md) | Exact component boundaries and contracts |

If an implementation intentionally departs from a lower-level rule, record the reason.
Do not let an accidental local style become precedent.

## 3. Principles

### 3.1 Continuity is sacred

The interface is a workspace, not a slideshow of server snapshots. Unrelated updates
must not dismiss a menu, close a select popup, move focus, reset a draft, replace a
selection, jump scroll position, or reload the page. Content that has not semantically
changed should retain its DOM identity.

- Give each piece of durable and transient state an explicit owner.
- Keep application state outside view instances that may be recreated.
- Reconcile background data locally; do not rebuild an entire surface for a small
  change.
- Separate foreground, interaction-blocking work from background synchronization.
- A background loading indicator must neither capture focus nor disturb layout.
- Animate causal transitions initiated by the user. Do not animate routine live-data
  churn or let animation temporarily compress usable content.
- Preserve unfinished work through reconnects, component updates, and recoverable
  errors.

Stability is part of correctness. A screen that is visually accurate but repeatedly
interrupts the user's current action is not finished.

### 3.2 Honor intent and remembered context

The interface should respect both the user's explicit request and the choices they have
already made. A focused change is not permission to redesign neighboring controls. A
navigation step is not permission to reset the workspace.

- Change the requested behavior while preserving established, unrelated appearance and
  interaction unless a dependency makes that impossible.
- Persist meaningful workspace preferences such as view mode, sort, pane sizes,
  visibility, and last-used choices at the scope where they apply.
- Preserve recoverable drafts and staged inputs across refresh or reconnection when
  losing them would interrupt thought or destroy work.
- Restore best-effort: stale projects, missing records, and obsolete stored values
  should degrade field by field instead of invalidating the whole session.
- Keep preferences contextual when the same choice has different meanings in different
  views.

Familiar context is an asset. Returning users should spend their attention on their
work, not on putting the interface back the way they left it.

### 3.3 Hierarchy comes before decoration

Users should understand the screen's regions, current context, and primary action in a
quick scan. Grouping should come first from alignment, spacing, typography, and shared
background—not from a border around every object.

- Establish one clear reading order and one dominant action per decision point.
- Let content fill the region intended for it; avoid decorative wrappers that make the
  real tool smaller.
- Every border, background, divider, badge, and nested card must communicate a useful
  distinction. Remove it if spacing or alignment already does the job.
- Keep controls near the content or state they affect.
- Use progressive disclosure for uncommon or advanced actions.
- Prefer concise labels and specific status text over ornamental explanation.

Visual simplicity is not emptiness. It is the absence of elements that compete with
the user's task.

### 3.4 Prefer directness over ceremony

The shortest understandable interaction is usually the best one. Do not require modes,
dialogs, confirmations, or explicit saves where direct manipulation and safe inference
can produce the same result.

- Autosave ordinary text edits after a short debounce; flush on blur and preserve the
  draft on recoverable failure.
- Let empty content advertise a single-click creation path and existing content use a
  familiar edit gesture with an accessible keyboard equivalent.
- Avoid a page-wide edit mode when individual fields can be edited in place.
- Infer the likely result first. Ask the user only when the outcome is genuinely
  ambiguous or consequential.
- Make equivalent gestures equivalent: a double-click, menu command, and visible Open
  action should reach the same operation where users reasonably expect them to.
- Use disclosure indicators only when there is something to disclose. A visual
  affordance must describe the behavior that actually follows.

Low ceremony does not mean hidden behavior. The surface still needs a discoverable
affordance, clear focus, and recoverable outcome.

### 3.5 Components encode shared meaning

Reuse a component when screens share its semantic role, anatomy, state model, and
interaction behavior. A shared component makes the product more learnable and lets a
fix improve every use. Consumers should configure documented variants rather than
override private descendants.

Do not abstract two controls merely because they happen to look alike. If their
selection rules, keyboard behavior, state ownership, or responsive behavior differ,
they may deserve separate components built from shared primitives. Reuse meaning, not
coincidental appearance.

Each component should have:

- one named purpose;
- stable anatomy and alignment anchors;
- one production implementation for the same feature wherever it appears, rather than
  parallel reader, inspector, dialog, or demo approximations;
- explicit variants rather than ad hoc consumer CSS;
- a complete state model, including empty, loading, error, offline, disabled, selected,
  focused, and unsupported states where relevant;
- presentation that truthfully distinguishes enabled, disabled, selected, pending, and
  completed behavior;
- semantic actions rather than direct knowledge of a provider or transport; and
- examples using the real production implementation.

### 3.6 Use a finite semantic vocabulary

Color, type, spacing, radii, elevation, and motion should express roles. They should not
be a trail of isolated numbers chosen inside individual components.

- Use semantic tokens such as primary text, secondary surface, destructive action, or
  selected row—not raw colors tied to one appearance.
- Use a deliberate type scale and spacing rhythm. Introduce a new step only when an
  existing step cannot express a recurring role.
- Name durable optical adjustments and explain the need; do not normalize unexplained
  one-off values.
- Use shared icon metaphors consistently. Size and align icons by their role, allowing
  small documented optical corrections where the artwork requires them.
- Ensure state is never communicated by color alone.

Consistency does not mean every object is identical. It means every difference has a
reason users can learn.

### 3.7 Responsive design reprioritizes; it does not merely shrink

At constrained sizes, protect the primary content, recognizable icons, text legibility,
and usable hit targets. Remove or relocate secondary information before shrinking the
interface below a usable scale.

- Make behavior container-aware when a component can appear in panes, drawers, and
  dialogs of different sizes.
- Decide intentionally which labels wrap, truncate, hide, or move. A popup option may
  wrap even when its compact selected value ellipsizes.
- Keep icons and hit targets from collapsing under flex pressure.
- Assign one clear scroll owner to each region.
- Test the supported minimum and a generous size, plus meaningful intermediate states.
- Preserve selection, focus, and scroll context across layout-mode changes.

Responsive success means the priority order remains clear—not that every desktop detail
is compressed into the same row.

### 3.8 Production behavior is the only valid proof

A catalog or wireframe can communicate intent, but it cannot substitute for the actual
component in its real composition. Demonstrations must use production components with
realistic fixtures and the same lifecycle they have in the application.

- Do not represent an interactive system with a decorative placeholder when its real
  behavior is material to the design.
- Enabled controls must work in the production composition. Otherwise disable them and
  explain why, or omit them until the action exists.
- Menus, dialogs, and popovers should begin in their normal state, open through their
  normal trigger, close normally, and be reopenable.
- Examples must cover meaningful variants and adverse states, not only the ideal case.
- Synthetic data should be realistic enough to expose clipping, wrapping, density, and
  state-transition problems.

The component catalog is a workbench for the product, not a second, more polished
implementation of it.

### 3.9 Be platform-fluent, not platform-costumed

Native clients should use native controls and behaviors. Web clients should translate
the platform's interaction principles using honest web primitives. They should not fake
system materials, title bars, or controls they cannot make behave correctly.

The product model can remain consistent across platforms while presentation adapts:
keyboard shortcuts, context-menu wording, file-manager names, window behavior, pointer
conventions, and system integrations should follow the host. Accessibility and user
expectations outrank superficial visual parity.

For Hot Sheet specifically, the native macOS app follows the HIG fully; browser and
Tauri clients are **HIG-shaped, not HIG-skinned**. The portable Kerf UI project should
retain that distinction for every platform it supports.

### 3.10 Performance is interaction design

The interface should acknowledge an action within the interaction frame even when its
full result takes longer. A delayed selection highlight or a frozen old view reads as a
missed click, not as careful processing.

- Project the immediate local consequence—selection, navigation, an inserted item, or
  a pending state—before unrelated asynchronous work.
- Keep blocking work out of pointer, keyboard, resize, drag, and animation frames.
- Load enough above-the-fold content to make navigation feel immediate, then append
  off-screen work progressively.
- Move substantial data processing to the server or a worker when it cannot be avoided;
  do not repeatedly derive the same expensive result during rendering.
- Bound caches, scrollback, subscriptions, observers, sockets, and retained media, and
  dispose them with the surface that owns them.
- Measure user-perceived milestones, not only request duration or eventual completion.

Fast feedback and eventual accuracy are complementary: show the intended state
immediately, then reconcile it without flicker when the authoritative result arrives.

### 3.11 Feedback should be quiet, local, specific, and truthful

Routine success should not interrupt. Problems should appear near the affected object,
say what happened, preserve recoverable input, and offer the next useful action.

- Show a blocking loading state only when the user truly cannot continue.
- Prefer determinate progress when the amount of work is known.
- Do not flash global indicators for routine synchronization.
- Keep errors actionable and distinguish unavailable, offline, unauthorized,
  incompatible, and invalid states.
- Distinguish unresolved, loading, empty, and failed states. Never announce an empty
  result before the authoritative result is known or show activity after work is done.
- Use confirmation for consequential or difficult-to-reverse actions; otherwise prefer
  immediate action with undo.
- Avoid feedback that moves controls or changes layout at the moment the user is trying
  to act.

### 3.12 Transient surfaces obey one spatial model

Menus, popovers, dialogs, sheets, galleries, permission requests, and similar surfaces
must behave as one coherent layer above ordinary content.

- Anchor a transient surface to the control or pointer that opened it and keep that
  relationship stable while surrounding content scrolls or rerenders.
- Keep the entire actionable surface within the viewport, using one owner for measured
  collision handling rather than stacking competing estimates.
- Define a shared layer order so the most consequential active surface is reachable;
  media, terminals, animations, and focus rings must not paint above it accidentally.
- Make dismissal predictable. Light-dismiss simple, non-destructive surfaces; keep a
  popup open while the user interacts inside it; use explicit Cancel or Close controls
  when dismissal is consequential or entered work needs protection.
- Standardize dialog headers, value tables, toolbars, footer actions, spacing, and
  lifecycle before creating local variants.

Opening a transient surface must not move the underlying page, change its selection, or
scroll its container.

### 3.13 Accessibility is a design input

Every important workflow must work with keyboard navigation, visible focus, assistive
technology, zoom, increased contrast, and reduced motion. Controls need accessible
names; reading order and DOM order must agree; useful text should remain selectable;
and targets must remain comfortably operable.

Accessibility review begins with component anatomy and state design. It is not a final
pass that can repair an incoherent interaction model.

### 3.14 Automation and AI remain under human control

Automation should reduce effort without obscuring authorship, consequence, or scope.

- Identify AI-authored content and distinguish it from human-authored content.
- Provide a direct non-AI path for essential operations.
- Describe background progress specifically enough that the user knows what is
  happening without watching it.
- Confirm consequential actions, and make ordinary generated output editable,
  retryable, or undoable.
- Be explicit about what information is sent, retained, or shared.
- Treat generated content as fallible: preserve the source, scope corrections, and
  never imply greater certainty than the system has.

## 4. Resolving common tensions

| Tension | Decision rule |
|---|---|
| Consistency vs. local context | Share the semantic component; add an explicit variant when the context genuinely changes its anatomy or behavior. |
| Density vs. clarity | Remove chrome and secondary labels before reducing type, icons, or hit targets below the system's usable scale. |
| Wireframe fidelity vs. responsiveness | Preserve hierarchy, relationships, emphasis, and actions; adapt exact geometry. |
| Native convention vs. cross-platform parity | Keep the same outcome and mental model; use the host platform's presentation and terminology. |
| Animation vs. continuity | Animate a user-caused transition only when it explains causality; never animate routine refresh churn. |
| Abstraction vs. independence | Extract when meaning and behavior align; do not couple components that merely share current styling. |
| Automation vs. trust | Make automated work visible, bounded, reversible, and non-disruptive. |
| Completeness vs. honest simplicity | Omit or visibly disable an unavailable action with a reason; never ship an enabled fiction. |
| Novelty vs. familiarity | Spend novelty on product-specific value, not on relearning standard controls. |
| Helpful expansion vs. requested scope | Preserve unrelated controls and styling; propose collateral redesign separately unless it is required for correctness. |
| Immediate response vs. authoritative data | Project the safe local consequence immediately, then reconcile without flicker. |

## 5. Implementation consequences for Kerf

This philosophy requires architectural discipline. It cannot be achieved only through
CSS polish.

### 5.1 State and rendering

- Declare the owner and source of truth for durable state, transient interaction state,
  and server state.
- Keep application state outside custom-element instances that can be recreated.
- Use stable keys and stable nodes so unchanged content retains identity.
- Keep rendering side-effect free. Post-render synchronization must be idempotent,
  scoped, and unable to create a render loop.
- Update the smallest meaningful subtree. Do not use whole-screen reconstruction as a
  synchronization strategy.
- Treat open menus, focused controls, text selections, drafts, and scroll offsets as
  state that unrelated renders must preserve.
- Model background activity separately from foreground blocking state.
- Persist user preferences and recoverable creation/feedback drafts in a versioned,
  scoped record; migrate or discard invalid fields independently.
- Batch related reactive writes so the user's new selection or view appears before
  deferred collection work begins.

### 5.2 Layout and motion

- Assign a deliberate layout owner and scroll owner to every region.
- Prefer structural layout rules over compensating offsets between unrelated elements.
- Animate containers independently from their content when sliding or resizing a
  region; do not squash the region's internal layout during the transition.
- Use directional push/pop motion to explain hierarchical navigation, moving complete
  adjacent stages without overlap; cross-fade stationary chrome when it changes.
- Respect reduced-motion preferences and make the final state correct without
  animation.
- Route dialogs, popovers, menus, and overlays through shared anchoring, collision,
  dismissal, focus, and layer-order contracts.

### 5.3 Component surfaces

- Put shared geometry and behavior in the component that owns the role.
- Expose named parts, properties, and variants; avoid consumer selectors that reach
  through a component's implementation.
- Maintain a production-backed component catalog with all meaningful states and
  responsive cases.
- Test components both in isolation and inside real screens, because composition bugs
  are product bugs.

## 6. Component contract

Before a new shared component is considered established, document or encode:

| Contract area | Question to answer |
|---|---|
| Purpose | What single semantic role does this component own? |
| Anatomy | Which parts and alignment anchors are stable? |
| State | What are its empty, loading, error, offline, disabled, selected, focused, and unsupported states? |
| Ownership | Who owns each durable and transient state, and what is authoritative? |
| Interaction | What pointer, keyboard, menu, and touch actions produce each semantic event? |
| Layout | How does it wrap, truncate, hide, resize, and scroll at supported dimensions? |
| Accessibility | What is its role, name, order, focus behavior, contrast, and non-color cue? |
| Feedback | Where do progress, validation, errors, confirmation, and undo appear? |
| Composition | In which real screens and neighboring states must it be proven? |
| Evidence | Which automated checks and inspected visual states demonstrate the contract? |
| Performance | What appears in the interaction frame, what is deferred, and which resources are bounded and disposed? |

Components should expose semantic events such as `selectTicket` or `openDiff`, not
transport operations. Provider capability decides whether an action is available; the
component communicates that result honestly.

## 7. Review protocol

Review the working interface, not only its source or isolated catalog entry.

### 7.1 Hard rejection conditions

A UI change is not ready if any supported workflow has:

- lost drafts, focus, selection, scroll position, or an open transient surface because
  unrelated state changed;
- an enabled control that does not perform its promised action;
- clipped, overlapping, unreachable, or illegibly compressed content at a supported
  size;
- inaccessible naming, order, focus, keyboard operation, contrast, or state cues;
- a demo that substitutes a placeholder for behavior under review;
- unexpected reloads, interval polling, render loops, or layout churn;
- an error state that blocks unrelated data or offers no useful recovery;
- a transient surface that is off-screen, detached from its trigger, behind lower
  content, or unpredictably dismissed; or
- a local change that silently redesigns unrelated controls or forgets established user
  preferences.

Do not average a hard failure away because the rest of the screen looks polished.

### 7.2 Review passes

1. **Intent and scope:** Can a reviewer name the user's goal, the primary action, and
   which neighboring behavior must remain unchanged?
2. **Hierarchy:** Does a three-second scan reveal regions, current context, and reading
   order without excessive boxes or labels?
3. **Behavior:** Exercise every visible action and the relevant state transitions.
4. **Continuity:** Repeat those actions while live/background state changes, refresh,
   and navigation occur; confirm preferences and recoverable drafts survive.
5. **Responsiveness:** Check the immediate acknowledgement and above-the-fold result
   under delayed data and large realistic collections.
6. **System fit:** Check component reuse, tokens, variants, transient-surface behavior,
   and contextual placement.
7. **Adaptation:** Check supported minimum and wide layouts, keyboard use, zoom,
   platform conventions, and reduced motion where relevant.
8. **Craft:** Inspect actual screenshots for alignment, rhythm, typography, truncation,
   optical balance, density, and unnecessary decoration.

Visual evidence must be opened and inspected by the implementer. A passing capture
command proves that an image was produced; it does not prove that the image looks
right.

### 7.3 Definition of done

For a material UI change:

- the real production composition implements the interaction;
- the component catalog exposes relevant variants through production components;
- state transitions and adverse cases have automated coverage proportional to risk;
- the supported minimum and a generous size have been visually inspected;
- keyboard, focus, accessible naming, contrast, and motion have been considered; and
- obvious issues discovered during review are fixed before requesting feedback.

Request human feedback for genuine product or aesthetic tradeoffs, not for visible
breakage that the implementation can already identify.

## 8. Lessons from Hot Sheet 2

The evidence audit covered the complete title inventory of 414 bug tickets and 169
requirement-change tickets, then reviewed the bodies and feedback histories of the
design-critique clusters in detail. The history shows repeated system-level themes, not
a collection of unrelated pixel defects. These examples are retained as provenance for
the philosophy.

| Recurring lesson | Representative tickets | Resulting rule |
|---|---|---|
| Shared roles need shared geometry | HS2-T2S6Q3, HS2-KB8XSD, HS2-X7WKV9 | Alignment and selection treatment belong to the role-owning component. |
| Structure matters more than decorative similarity | HS2-5WZXME, HS2-J9Y0J3, HS2-J23P84 | Preserve region hierarchy and consistent content anchors before polishing. |
| Excess chrome obscures the tool | HS2-XRVA64, HS2-GCZZ6F, HS2-5SS0ZE | Remove redundant boxes, footers, and framing; let primary content occupy its region. |
| Responsive behavior must express priorities | HS2-RA3TRX, HS2-3ME1T2, HS2-STVA92 | Protect legibility, icons, and targets; choose wrapping, truncation, and scrolling deliberately. |
| Reuse is semantic, not cosmetic | HS2-DM4STX, HS2-09F07P, HS2-2P9K4Y, HS2-G7P7S7 | Use the same implementation for the same feature, but keep controls independent when their semantics differ. |
| A polished demo is not product proof | HS2-VW5EW4, HS2-R2FGZ5, HS2-Z0M2VV | Use production components, realistic fixtures, normal triggers, and working actions. |
| Live UI continuity is a correctness requirement | HS2-AYSQ1C, HS2-M3CDNG, HS2-GGE650, HS2-KP0HQ8, HS2-DBE4BF | Stable identity and explicit state ownership must protect the user's current action. |
| The workspace should remember the user | HS2-3QXHF3, HS2-8H7F72, HS2-W3FDPM, HS2-BH8ZVD | Restore preferences, drafts, and contextual choices best-effort instead of resetting them. |
| Editing should be direct and low-ceremony | HS2-NZRBHY, HS2-BE8A3V, HS2-X3GX39, HS2-JZTVKC | Prefer inline editing, autosave, familiar gestures, and questions only for real ambiguity. |
| Perceived latency changes meaning | HS2-NCT01M, HS2-W52RER, HS2-6PPVJC, HS2-S2C5RY | Acknowledge intent immediately, render progressively, and keep interaction frames free of heavy work. |
| Transient surfaces are a system | HS2-RN6X3E, HS2-SWC9E4, HS2-MVJ85W, HS2-35N9RS, HS2-WQE7BR | Share viewport collision, anchoring, layering, focus, and predictable dismissal rules. |
| Unknown is not empty and busy is not done | HS2-CSYQHX, HS2-E2AZ2N, HS2-N718B0, HS2-RHQATM | State copy and affordances must describe the true lifecycle and available behavior. |
| Visual choices need a finite vocabulary | HS2-66M88K, HS2-RZA0H3 | Use semantic tokens and a deliberate type/spacing scale instead of arbitrary local values. |
| Actual evidence must be inspected | HS2-B2KPNW, HS2-G4G95R, HS2-SK42H7 | Review wide and constrained screenshots for visible defects, not just test completion. |
| Placement and timing carry meaning | HS2-8DYEQM, HS2-9QCCH3, HS2-FW96JM | Put actions by what they affect and reveal transient UI only when requested. |
| Scope fidelity protects learned behavior | HS2-7CTQJC, HS2-CV0J2E, HS2-E729WG | Solve the requested problem without gratuitous redesign; simplify only where the workflow calls for it. |

Clusters of automated UI-stability reports should first be treated as evidence of a
shared ownership, identity, focus, or render-lifecycle defect. Fix and suppress the
systemic cause rather than styling each symptom independently.

## 9. Starting the standalone Kerf UI project

Carry sections 1–7 into the new project as durable guidance. Then:

1. Replace Hot Sheet-specific component names with the new project's domain vocabulary.
2. Define the supported platforms and minimum viewport/window sizes.
3. Choose the project's semantic color, type, spacing, radius, and motion scales before
   component-local values proliferate.
4. Establish a production-backed component catalog and visual-review workflow early.
5. Define stable state ownership and reconciliation rules before building live-data
   surfaces.
6. Keep this Hot Sheet lesson table as provenance or move it into an architecture/design
   decision record.

The new project can develop a distinct visual identity. It should not discard the
behavioral disciplines that make that identity trustworthy.
