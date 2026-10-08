# AI note feedback synthesis (HS2-355565)

## Current behavior

The web client's thumbs controls save a regular ticket note beginning
`AI feedback for <target>: Helpful` or `AI feedback for <target>: Not helpful`, followed
by an optional explanation. For a `note:<id>` target, the Notes UI nests the rating
inside that source note behind a disclosure and Git-backed parent deletion removes the
rating. Missing-parent and other-target feedback stays visible. The action preserves an
auditable response in the ticket; it does not update a model, prompt, agent instruction,
or future suggestion. Git-backed ratings carry structured target, value, and optional
explanation metadata; note id, actor, creation time, and edit time supply identity and
provenance. Repeating a rating from the same rater and target revises its note; a
withdrawal retains the note with a null rating. The Git commit history retains prior
versions. Exact legacy prose ratings are normalized on read and marked `legacy` in
the feedback query. See [the client
contract](06-clients.md) and [ticket note format](17-ticket-file-format.md).

`GET /providers/{connection_id}/ai-feedback` returns the current feedback source
ledger for a Git connection, including completed tickets. Each row identifies the
connection, ticket and feedback note; target, rating, explanation, rater, creation
time, edit time, and whether it came from legacy prose. Unsupported connections
return an explicit capability error. A structured rating is written through ticket
`PATCH` with `ai_feedback: {target, rating, explanation?}` and an actor with a
stable id; `rating: null` withdraws it. Feedback must be patched separately from
other ticket fields. Repeated writes by the same rater and target update one note.

## Recommended process

**Owner and trigger.** The repository maintainer owns the accepted guidance. Once a
month, or on demand after a notable cluster of feedback, a human starts a local
synthesis run. If no new relevant notes exist, it reports that and makes no edit.
This is a review cadence, not a background network job. A later headless command can
prepare the draft (HS2-SX1F4J); it cannot approve or publish it.

**Source scope.** Read feedback notes from the ticket-provider connections explicitly
included by the maintainer for this project, including completed tickets. Include the
rating, optional explanation, target note/activity, ticket identity, and note id/time.
Do not treat ordinary `FEEDBACK NEEDED` notes, completion confidence, or generic
comments as votes. For the initial prose-only data, match the exact known prefix and
show uncertain records for human triage. Report providers whose capabilities prevent
retrieval, rather than treating their absence as zero feedback. Keep the review window
and provider list visible in the draft.

**Privacy and provenance.** Process locally within the configured project and only send
source text to the AI provider deliberately chosen for the draft. The draft may contain
private ticket content; keep it untracked until reviewed. Strip credentials, personal
identifiers, paths tied to an individual's machine, and unnecessary excerpts before
committing any summary. An accepted theme links to ticket/note IDs where access
permits, without copying private text into a broadly shared document. Do not pull
feedback from other projects or accounts by default.

**Deduplication and interpretation.** Use `(provider connection, ticket ID, feedback
note ID)` as the source key. Corrections or withdrawals supersede an earlier rating;
repeated clicks by the same person on the same target do not become independent
evidence. Group by the _requested behavior and context_, not by the thumb alone.
Seek at least two independent examples before promoting a recurring preference to
general guidance. A single high-impact failure becomes a ticket for direct repair,
not a new global rule. Positive and negative evidence can coexist; state the boundary
under which each applies. Count distinct source notes and distinguish a theme from a
one-off request. Do not infer silent approval from a lack of ratings.

**Review and publication.** Produce a concise draft with: review period and source
coverage; up to five themes, each with an action, scope, evidence count, and source
references; conflicting or uncertain evidence; proposed ticket links; and a last
reviewed cursor. The maintainer edits or rejects each proposed theme and commits only
accepted, broadly useful guidance to a short repository document or the relevant
existing requirement. Keep raw feedback in its ticket, not in the guidance file.
Open implementation or defect tickets for concrete changes. Store the source ledger
and cursor locally or in a restricted project record so a rerun is idempotent; a
committed summary needs only its review date and source coverage. Publication should
be a normal reviewed repository change.

**Agent consumption.** Future agents read the accepted repository guidance through
the normal project instructions and relevant docs when planning work. They may use
the source ticket links to understand context, but do not treat a single vote or an
unreviewed draft as binding. More specific ticket requirements and current user
instructions still govern the task. The guide should stay short and be revised or
removed when later evidence contradicts it.

## Follow-up delivery

- **HS2-2G2336:** structured, revisable ratings and feedback query (delivered).
- **HS2-SX1F4J:** build the local draft, deduplication, redaction, cursor, and human
  review flow, with tests. It depends on the structured source contract.

Until HS2-SX1F4J ships, a maintainer can query feedback, inspect uncertain legacy
records, and write a reviewed summary using the rules above.
