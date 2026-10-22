<!-- hotsheet:begin section=ticket-driven-work v=6 -->

## Ticket-Driven Work

Create a Hot Sheet ticket before substantial work described directly by a user, including features, bugs, refactors, and multi-step code changes. Claim it before editing, keep its phase and ETA current, and release the claim whenever work stops. A claim is the live activity signal; `claim_count` records past attempts and must not be used to infer current activity.

- Create follow-up tickets immediately for unfinished steps, open decisions, known gaps, and designed but unbuilt behavior. Include their slugs in the current ticket's completion note.
- Use `FEEDBACK NEEDED` only when the current ticket cannot proceed without a user decision or unavailable external state. Leave it Started, describe the blocker, and release its claim. Status reports, release waits, and sandbox blockers are regular notes, not feedback requests; close tickets you created yourself; give a visual tradeoff captures and `CHOICE` options; check open tickets and the code before asking.
- Before completing, verify the scope, update tests, coverage and docs, scan for placeholders and incomplete behavior, and record the result, verification, and every follow-up.
- When a UI ticket has a problem or design image, capture and attach an after image of the matching component, state, and viewport. Explain any unavailable capture in the completion note.
- Name every addressed ticket slug in its commit message.
- Re-read the whole ticket, including notes added since your last read, before every action on it, and quote the note you are acting on in your result.
- Change only what the ticket asks and preserve neighboring appearance and behavior; propose collateral changes as separate tickets. When the ticket names an attachment, reference art, a prior implementation, or another project, open it before designing.
- Measure before restructuring: record the number that justifies a performance or architecture change, and close the ticket as not worth it when the number is small.
- Record `activity` notes when a meaningful subtask starts and finishes; keep repeated or reversed transitions as separate history entries. Put conclusions, decisions, and recommendations in `regular` Markdown notes, with a short activity entry pointing to them when useful.
- Give each AI-authored activity note a one-line, outcome-oriented `note_summary` in the same update as its Markdown body. Keep the summary plain text and preferably under 80 characters; put implementation and verification detail in the body.
- For multiline CLI Markdown, use `--note-file <path>` (or `--note-file -` for stdin) with real line breaks. Use `--allow-literal-backslash-n` only when literal backslash-n prose is intentional.

<!-- hotsheet:end section=ticket-driven-work -->
