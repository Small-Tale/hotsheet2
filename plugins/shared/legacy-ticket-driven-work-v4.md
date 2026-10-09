<!-- hotsheet:begin section=ticket-driven-work v=4 -->

## Ticket-Driven Work

Create a Hot Sheet ticket before substantial work described directly by a user, including features, bugs, refactors, and multi-step code changes. Claim it before editing, keep its phase and ETA current, and release the claim whenever work stops. A claim is the live activity signal; `claim_count` records past attempts and must not be used to infer current activity.

- Create follow-up tickets immediately for unfinished steps, open decisions, known gaps, and designed but unbuilt behavior. Include their slugs in the current ticket's completion note.
- Use `FEEDBACK NEEDED` only when the current ticket cannot proceed without a user decision or unavailable external state. Leave it Started, describe the blocker, and release its claim.
- Before completing, verify the scope, update tests, coverage and docs, scan for placeholders and incomplete behavior, and record the result, verification, and every follow-up.
- When a UI ticket has a problem or design image, capture and attach an after image of the matching component, state, and viewport. Explain any unavailable capture in the completion note.
- Name every addressed ticket slug in its commit message.

<!-- hotsheet:end section=ticket-driven-work -->
