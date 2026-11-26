---
name: ui-ux-review
description: Ruthless Principal-Designer UI/UX review of one named screen, flow, or rendered document, judged against the project's stated design intent and the design system's house grammar, with real captures, an independent second opinion, a reconciled synthesis, and Hot Sheet tickets for every finding.
allowed-tools: Read, Grep, Glob, Bash, Agent
---

<!-- ai-playbook:skill name=ui-ux-review v=1 -->

Use this when the maintainer asks to review the UI or UX of a surface, after changing a
user-facing surface, or as the design half of any UX-affecting change. It applies one fixed
review lens, gets a second independent opinion, reconciles the two, and turns findings into
Hot Sheet tickets. It judges a *screen or flow* against design heuristics; correctness belongs
to the tests, and the `ui-evaluation` rule says what passing tools do not prove.

## 1. Prepare, in this order, before writing a word of critique

1. **Learn what the design is trying to be.** Read the project's UI conventions document (the
   one the `hotsheet-requirements-documentation` rule requires) and the design system's own
   guidance it points at (for Kerf, the house grammar named by `kerf-ui-interaction-defaults`).
   Critique against **that intent**. "Add a hero metric card" is not feedback; it is a
   different product. If you think the intent itself is wrong, say so as its own finding
   rather than smuggling it in as a detail.
2. **Learn what is actually built.** The requirements summary carries status markers, and a
   feature-health document, when the project keeps one, says what actually works. Flagging a
   design-only or deferred feature as a design failure wastes the review; flagging a shipped
   one that reads as unfinished is the point.
3. **Name the surface or flow in scope.** A full-app sweep is many reviews; do them one screen
   or flow at a time and say what you covered. The UI document is the map of what each screen
   is supposed to do; read its section first.
4. **Decide how you will see it**, in this order of preference, and say which you used:
   - a live look at the running app, read-only: never create, edit, or delete the maintainer's
     real data, and never restart their instance;
   - an isolated instance with representative seed data and its own data directory, for
     anything that needs seeding or clicking;
   - the project's scripted capture command or its end-to-end harness, for repeatable captures
     of a specific flow;
   - source only, when nothing can render; then say the review covers structure, copy, and the
     interaction contract, not pixels.
5. **Capture the real thing.** Wide, medium, and narrow widths including the wide-to-narrow-
   to-wide transition, light and dark schemes, and every state the flow has (empty, loading,
   error, populated, selected). Follow the `playwright-operations` captures rule when it is
   installed: runner output directory, settled state with animations disabled, attach to the
   ticket, never commit. Then **look at the images**; a critique written from the component
   source is a code review wearing a hat.

For a **Markdown or README** target, read the source and reason about the rendered result:
heading tree, section order, first-screenful hook, list density, code-block length, table
width, image alt text, link labels, and the install-to-first-run path.

## 2. The review lens (use this prompt verbatim as the standard)

> Act as a ruthless, elite Principal Product Designer and UX Researcher. Review the interface.
> Evaluate the design strictly using industry heuristics, human-computer interaction
> principles, and modern design systems. Do not just praise the aesthetic; provide deep,
> critical feedback across these exact categories:
>
> - **Visual Hierarchy & Layout**: focal points, grid alignment, whitespace, scanning patterns
>   (F-pattern or Z-pattern), and whether primary actions stand out from secondary elements.
> - **UX Friction & Cognitive Load**: unnecessary steps, confusing user flows, ambiguous icons,
>   hidden navigation, or cognitive overload on this screen.
> - **Accessibility (a11y) & Readability**: contrast ratios (WCAG), target sizes, font sizing
>   and scaling, text legibility.
> - **Microcopy & Content Strategy**: button labels, helper text, empty-state messaging, error
>   states: clarity, tone, actionability.
>
> Format the response: **The Good** (what works, keep it) · **Critical Issues** (ranked highest
> to lowest severity by user impact) · **Actionable Recommendations** (specific, practical
> fixes; no guessing).

Weight the categories for the project's platform: a desktop app weighs pointer target size,
keyboard navigation, focus-visible states, and hover and drag affordances over mobile tap
targets; a web app that also ships in a native shell must behave the same in both, so flag a
flow that works in the browser but would break in the shell.

## 3. Workflow

1. **Name the surface or flow** and how you viewed it (live, isolated, scripted capture,
   source only).
2. **Do your own review** with the lens. Be concrete: cite screens, components, states, and
   README lines, not vibes.
3. **Get a second, independent opinion and compare notes.** If the environment has a second
   model tool, ask it the same prompt on the same captures; otherwise spawn a fresh
   general-purpose subagent (not a fork, which would inherit your view) with the verbatim lens
   and the captures. Note which source you used. Reconcile: keep findings both raise (high
   confidence), weigh disagreements on merit rather than averaging, and drop anything neither
   the evidence nor the second reviewer supports.
4. **Write the synthesis** in the exact format: The Good · Critical Issues (ranked) ·
   Actionable Recommendations. Say where the two reviews disagreed.
5. **File Hot Sheet tickets** for the critical issues and the recommendations, carrying the
   human's exact component, placement, state, and viewport nouns. Reference the review and the
   date. Group tightly related fixes into one ticket; keep separable ones apart so they can be
   prioritized. A fix that is a taste, brand, or judgment call gets a `FEEDBACK NEEDED` note
   asking the maintainer before anyone implements it.
6. **Do not implement in this pass** unless the maintainer said to. When later asked to apply a
   recommendation, do it under its ticket and re-review the result.

## Hard rules

- **Concerns become tickets, not silent edits.**
- **Don't overclaim.** Never let review copy present a design-only or underbaked feature as
  shipped, and do not invent numbers or limits.
- **Never disrupt the maintainer's live instance.** Read-only there; use an isolated instance
  for anything interactive.
- **Published surfaces stay self-contained.** Follow the project's rule on ticket markers in
  published prose and its spelling convention.
- **Attach the inspected captures** to the ticket as evidence; a capture that shows a defect
  is problem evidence, never correctness evidence.
