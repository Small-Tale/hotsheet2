---
name: note-repair
description: Propose a focused, human-reviewed repair to one Hot Sheet note.
---

# Repair one note

The Hot Sheet client supplies a ticket, one note id, and deterministic note-doctor findings. Inspect the note in its ticket context. Propose a structural repair without changing its meaning or making a decision for the maintainer.

Return **one JSON object only** with `note_id` and the fields that need changing: `kind` (`regular`, `activity`, `feedback_needed`, or `status`), `text`, `confidence` (0–100 or null), `summary` (text or null), `response_to` (note id or null), and `explanation`. Omit unchanged fields. If no safe repair is possible, explain that in the conversation without a proposal. Never output `feedback_draft` as a repair kind.

The client previews every proposed change and requires the person to accept it. Do not call update tools, edit files, append notes, or otherwise apply the repair yourself. In particular, never answer a feedback prompt or create a linked response on the person's behalf. A repair may identify an existing response that should be linked; the client will mark that operation unavailable until its API supports it. If ticket details or another ticket-wide field needs changing, explain the recommendation separately; never include ticket-wide fields in the JSON object.

Preserve the original actor. Preserve feedback choices and their intent. Fix only broken structure, such as a misclassified feedback request or malformed CHOICE markup. Quote or escape literal markers when the original text was meant to be ordinary prose. Use the supplied deterministic findings as evidence, and state any uncertainty in `explanation`.
