# 32 — Notifier Plugins

**Status:** Shipped. The core contract, the `speak`, `ntfy`, and `email` built-ins,
external plugins, rules, the server router, REST administration, and the headless CLI
shipped in HS2-R5HTZN. The `webhook`, `slack`, `teams`, and `twilio` built-ins shipped in
HS2-97ZHMM. App Settings → Notifications in the web client shipped in HS2-DPBJJA (§32.9.1).

Hot Sheet raises events that a person may need to hear about when they are not
looking at the app: a ticket finished or became blocked, an AI asked for feedback or
permission, a project's AI work stopped (HS2-2MM07G), a periodic digest is ready
(HS2-D33WPH). A **notifier** is a plugin that delivers such an event to somewhere
outside the app: the computer's voice, a phone, an inbox. Notifiers are a plugin type
separate from AI-tool plugins ([05](05-ai-tool-plugins.md)). In-app toasts and OS
notifications remain the client's job ([06](06-clients.md), HS2-ZP869N).

The implementation is the `hotsheet-notify` crate. The server and `hotsheet-cli` both
drive it, so every channel can be set up, tested, and used headless.

## 32.1 Concepts

| Term     | Meaning                                                                                                 |
| -------- | ------------------------------------------------------------------------------------------------------- |
| Message  | A provider-neutral notification: event kind, title, Markdown body, severity, routing facts, attachments |
| Notifier | A plugin: a capability descriptor, a typed settings schema, and a delivery function                     |
| Channel  | A configured notifier instance with its settings, for example "my phone" (`ntfy`, topic `x`)            |
| Rule     | Routes matching events to one or more channels                                                          |
| Delivery | One channel's attempt to send one message, recorded in the delivery log                                 |

## 32.2 Messages and events

A message carries:

- **`event`**: one of `ticket.status_changed`, `ticket.feedback_needed`,
  `ticket.blocked`, `permission.requested`, `notification`, `digest`, `project.stopped`,
  `test`, or `notifications.held` (a channel's summary of held deliveries, HS2-CFMSP2);
- **`title`** and a CommonMark **`body`**;
- **`severity`**: `info` < `success` < `warning` < `error`;
- the **project** (a checkout id, store id, or project path), the project's display
  name, and the store;
- **ticket facts** (slug, status, previous status, category, priority, tags). Rules
  filter on these;
- an optional **click URL**, **attachments** (name, MIME type, bytes), and a
  **dedupe key**.

## 32.3 Capabilities and settings

Every notifier publishes a descriptor (`GET /notifiers`, `hotsheet-cli notify notifiers`):

- **Body formats** it accepts: `markdown`, `html`, `plain`.
- **Byte limits** for the title and the rendered body. A title limit of 0 means there is
  no separate title field.
- **Attachments**: maximum count, per-file bytes, total bytes, and image-only.
- **Priority**: the notifier maps severity to its own priority scale.
- **Click URL**: the notifier can attach a link outside the body.
- **Audio**: the output is spoken.
- **Settings schema**: typed fields (`string`, `list`, `number`, `bool`, `choice`,
  `secret`) with labels, defaults, help, and required flags. A client renders its form
  from this schema without knowing the notifier.

`secret` settings never hold a value. The configuration stores
`{"secret": "<key name>"}`, and the value lives in the machine key registry (OS
credential store, [04](04-core-server-cli.md#471-secure-provider-keys)). It is
resolved only for the duration of one delivery. A `HOTSHEET_API_KEY_<NAME>` environment
variable overrides the value, as for provider keys. Because a credential is stored after
its channel exists, a channel may be saved with a required credential still missing.
Delivery then fails and names the `notify channel secret` command, and `channel add`
prints that command.

## 32.4 Rendering and degradation

Before delivery, each message is rendered for the channel's capabilities:

1. **Format.** Markdown is passed through when accepted. Otherwise it is converted to
   HTML (with raw HTML neutralized, because ticket text may be quoted) or to readable
   plain text that keeps list structure and shows link URLs in parentheses. Email also
   gets a plain-text alternative.
2. **Title.** A notifier without a title field gets the title as the first line of the
   body (spoken as "Title. Body").
3. **Links.** Without a click-URL field, the URL becomes an "Open in Hot Sheet" link in
   the body. Audio drops URLs entirely.
4. **Attachments.** Files that exceed the count, size, total, or image-only limits are
   dropped. The body then says "N attachments omitted: names", so the reader knows.
5. **Truncation.** Text over a byte limit is cut on a UTF-8 boundary and ends with
   "… (truncated)". HTML is truncated from its Markdown source, so tags stay balanced.

The delivery record notes `truncated` and `omitted_attachments`.

## 32.5 Configuration

`${HOTSHEET_HOME}/notifications.json` is machine-local and holds no secrets:

```json
{
  "version": 1,
  "channels": [
    {
      "id": "phone",
      "notifier": "ntfy",
      "label": "My phone",
      "enabled": true,
      "settings": { "topic": "hs-7f3k", "token": { "secret": "notify-phone-token" } }
    }
  ],
  "rules": [
    {
      "id": "attention",
      "events": ["ticket.feedback_needed", "permission.requested"],
      "channels": ["phone"],
      "filter": { "min_severity": "warning" },
      "cooldown_seconds": 600
    }
  ]
}
```

- Channel and rule ids are lowercase letters, digits, `-`, and `_`.
- A rule matches when it is enabled, names the event, and its filter passes. All of the
  filter's lists are optional:
  - `projects` matches the project, the store id, or any checkout linked to the store,
    so one checkout id covers its ticket, permission, and project events;
  - `tags` matches any one tag;
  - `statuses` matches the new status;
  - `categories` and `priorities` match the ticket's values;
  - `min_severity` sets the lowest severity that passes;
  - `text` matches a case-insensitive substring of the ticket slug, title, or a tag;
  - `views` lists saved views as `{"checkout": "<checkout id>", "view": "<view id>"}` and
    matches a ticket in any one of them (HS2-G8NXNC, below).
- Any ticket filter, including `text` and `views`, requires ticket facts.
- **Saved-view filters (HS2-G8NXNC).** The process that notices a ticket change (the
  server router, or a serverless `hotsheet-cli` or `hotsheet-mcp` write) looks up each
  view an enabled rule names among the checkouts linked to the ticket's store, tests the
  ticket against the view's query (`hotsheet_ticketing::saved_views`), and records the
  views it is in on the message's ticket facts. A view that was deleted, or a checkout that
  is not linked to the store, contains nothing. The query language is the web client's:
  - `AND`, `OR`, `NOT`, and parentheses, with implicit `AND`. Without any of them the
    plain words form one phrase, as in the search bar; with an operator, each word is a
    term.
  - `group:` (one group by name, without regard to case or spacing; quote a name with
    spaces) and `has:group` (HS2-44W3TP).
  - `tag:`, `is:` (`up-next`, `active`, `needs-review`, `open`, `closed`, `duplicate`,
    and the statuses), `has:attachment`, `has:media-annotation`, `attachment:` with `*`
    globs, and `created|updated|completed|verified|started|archived-before|after:` dates.
  - Any other term is a case-insensitive substring of the slug, title, tags, details,
    notes, or attachment names.
  - Differences from the client: a date written without a zone is read as UTC, and
    locale-ordered dates are text. `has:`, `attachment:`, and date terms inside a Boolean
    expression are read structurally here, where the client still reads them as text
    (HS2-DE9CMP).
  - `has:commit` (HS2-VPG8XA): the server router answers it with the same repository scan
    as the checkout listing (the checkout's code repository has a commit whose subject or
    `Refs:` line names the ticket), run only when the term is evaluated. Serverless CLI and
    MCP writes have no repository scan, so for them `has:commit` never matches.
- A channel may set `quiet_hours` (`start` and `end` as `HH:MM`, a `time_zone`, and
  `allow_errors`, default true) and `max_per_hour`. `time_zone` is an IANA name such as
  `America/New_York`, so daylight saving changes apply automatically (HS2-9T27WQ); an
  unknown zone is refused. A window without a zone uses the fixed `utc_offset` (such as
  `+09:00`, default `+00:00`), which stays supported for configs written before zones.
- `test` and `notifications.held` cannot be rule events; tests go only to the channel
  being tested, and a summary only to the channel that held the deliveries.
- Saving validates everything: unique ids, installed notifiers, settings against the
  schema, rule references, and no inline secrets. An invalid change is refused and
  nothing is written.
- Writes are atomic (temp file plus rename). Each read-modify-write holds an OS file
  lock on `.notifications.json.lock` beside the file, so concurrent CLI and server edits
  never drop each other's change (HS2-YGNDET).
- A newer `version` than this build understands is refused rather than rewritten.

## 32.6 External notifier plugins

A third party adds a notifier without changing Hot Sheet. The notifier is a directory
`${HOTSHEET_HOME}/notifiers/<id>/` with a `manifest.toml`:

```toml
id = "slack"
display_name = "Slack"
description = "Posts to a Slack channel."
command = ["notify.js"]        # run without a shell; a relative file in this directory runs from here
timeout_seconds = 30

[capabilities]
formats = ["markdown", "plain"]
max_title_bytes = 150
max_body_bytes = 3000

[capabilities.attachments]
max_count = 0

[[settings]]
key = "webhook_url"
label = "Webhook URL"
type = "secret"
required = true
```

For each delivery, Hot Sheet runs the command and writes one JSON envelope to stdin:

```json
{
  "protocol": 1,
  "notifier": "slack",
  "channel": { "id": "team" },
  "settings": { "webhook_url": "<resolved>" },
  "message": {
    "event": "ticket.blocked",
    "severity": "warning",
    "title": "…",
    "body": "…",
    "format": "markdown",
    "plain_alternative": null,
    "url": null,
    "attachments": [{ "name": "a.png", "content_type": "image/png", "bytes": "<base64>" }],
    "project": "…",
    "project_name": "…",
    "ticket": { "slug": "HS2-…" },
    "truncated": false,
    "omitted_attachments": 0
  }
}
```

The plugin prints a result line on stdout, either `{"ok": true, "id": "<optional>"}` or
`{"ok": false, "error": "…", "retryable": false}`. Earlier stdout lines are ignored, so
logging is safe. Exiting without a result line is a retryable failure, and the stderr
tail is reported. A plugin that outlives its timeout is killed.

- Resolved credentials travel only on stdin, never in argv or the environment.
- Built-in ids (`speak`, `ntfy`, `email`) win a collision.
- A plugin directory that fails to load is listed in `load_errors` and does not hide
  the others.
- **Approval (HS2-208RB1).** Creating a channel that names an external plugin approves
  the plugin as it is then: the channel records a SHA-256 digest of every file in the
  plugin directory (`plugin_digest`). Before each delivery, Hot Sheet recomputes the
  digest. If any file was added, removed, renamed, or edited, or the channel has no
  approval, the delivery fails without running the plugin. The error names
  `hotsheet-cli notify channel trust <id>`.
- Re-adding a channel keeps its approval, so a scripted, idempotent `channel add` never
  re-trusts changed code. Only `channel trust` (or `POST
/notification-channels/{id}/trust`) approves the current content. `notify config`
  marks channels whose plugin changed. Built-in notifiers need no approval.

## 32.7 Dispatch

- **Matching.** Every enabled rule that matches contributes its channels. Each enabled
  channel receives a message once, even when several rules match. An explicit send or
  test goes to one channel whatever its rules or enabled flag.
- **Quiet hours and rate limits (HS2-4JG95G).** Inside a channel's quiet window, a
  rule-routed delivery is recorded as `suppressed` with the reason; error-severity
  messages still go out while `allow_errors` holds. Past `max_per_hour` deliveries in
  the last hour, a delivery is `suppressed` with the reason. The count reads the
  delivery log, so the limit survives restarts. Tests and explicit sends bypass both.
  Such deliveries are logged with `held: true`; a cooldown repeat is not.
- **Held summaries (HS2-CFMSP2).** Once a channel is no longer holding (its quiet window
  has ended and it is under `max_per_hour`), it receives one `notifications.held` message
  for everything held since its last summary: "4 notifications were held", then a line
  per event kind with its count and up to ten ticket slugs, and the reasons. The log keeps
  identifiers only, so the summary names no ticket titles. Each dispatch flushes first, so
  the summary arrives before the next message (this covers serverless CLI and MCP writes),
  and the server also flushes every minute. The summary is logged like any delivery and
  is the watermark for the next one, so it survives restarts. It does not count toward
  `max_per_hour`. A failed summary is logged and not resent. Without a delivery log (a
  host with no machine home) nothing is kept to summarize.
  **Process coordination (HS2-MFD44D):** one process reserves a summary before calling
  its notifier. A concurrent flush skips the busy reservation; ordinary writes continue,
  including a ticket write synchronously triggered by a custom notifier. The private
  journal entry records the exact held delivery IDs, so a new hold arriving during a
  send remains pending for the next summary. Reservation persistence is synced before
  transport; a lock, journal or reservation error prevents that summary from being sent.
  After process exit, the next flush records an unresolved reservation as failed with
  an explicit unknown/interrupted outcome and does not resend it, even if its channel
  was disabled or removed. This is at-most-once summary admission, not a guarantee of
  exactly-once provider delivery: an interrupted send may or may not have reached the
  provider. Bounded transport retries retain their existing semantics. Legacy delivery
  records remain readable; tagged private reservation entries are not public delivery
  records. Process-shared journal locks serialize append/compaction, retaining any
  reservation needed by a retained held/final record and every unresolved reservation.
  All active dispatchers must use the updated implementation; an older running binary
  does not participate in these locks.
- **Cooldown.** With a dedupe key and `cooldown_seconds > 0`, a repeat to the same
  channel inside the window is recorded as `suppressed` and not sent. The check reads
  memory and the delivery log, so it survives a server restart and separate CLI runs.
  Only successful deliveries start a cooldown.
- **Retry.** Retryable failures (network, timeouts, HTTP 408/425/429/5xx, SMTP
  transient codes, plugin crashes) are retried after 2 s and 8 s. Permanent failures
  (bad settings, rejected credentials, other 4xx, a plugin's `retryable: false`) are
  not retried. A missing credential or uninstalled notifier fails without sending.
- **Delivery log.** `${HOTSHEET_HOME}/notification-deliveries.jsonl` keeps one record
  per delivery:
  - ids, time, event, channel, notifier, and rule;
  - outcome, attempts, the error, and the provider message id;
  - project and ticket slug, truncation, and omitted attachments;
  - a 64-bit hash of the dedupe key.

  It never holds a title, body, recipient, or attachment. Past 1,000 lines it is
  compacted to the newest 500.

## 32.8 Server event sources

The server's notification router subscribes to the change bus that already carries
every server write and every store-watcher change, including CLI and MCP writes while
the server runs. It routes each event on a blocking worker, so a slow channel never
delays the bus:

| Bus event                              | Notification event                                                                                               |
| -------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| ticket `created`/`updated`/`changed`/… | `ticket.status_changed`, `ticket.feedback_needed`, `ticket.blocked`, each only on the transition into that state |
| `permission_asked`                     | `permission.requested` (warning; dedupe per request)                                                             |
| `notification` (`POST /notifications`) | `notification`: first line as title, the rest as body, severity and dedupe key carried                           |

The bus carries ids, not before/after states. Transitions are found by comparing the
changed ticket with a snapshot that is seeded from each store's index when the router
starts, before it reads the bus. The first change in a store hosted after startup only
records its baseline. An event with no enabled rule is dropped before any ticket is
read.

**Without a server (HS2-8VAXXA).** When no live server hosts a store, a
`hotsheet-cli` ticket write delivers its own ticket transitions. A live server is one with
an `instances` record for that store root and a live PID. The CLI snapshots the index rows
before the command, diffs them after, and builds messages with the same
`hotsheet_notify::transitions` the server router uses. When a server does host the store,
the CLI stays quiet and the server's watcher routes the write, so nothing is sent twice.
Read-only commands are not watched. A delivery failure is reported on stderr and never
fails the ticket command. `project.stopped` needs the server's timers. The serverless
`hotsheet-mcp --path` backend wraps its writes with the same watch (HS2-FYZ5S9): a
checkout-scoped or provider-qualified tool call is watched once, on the store it resolves
to, and MCP reads are never watched. An MCP shim that proxies a server is already covered by
the server router.
Ticket, permission, and project events carry the checkout ids linked to their store,
and a linked checkout's alias is the project name in the message (HS2-4JG95G).

`digest` (HS2-D33WPH) is reserved for that producer. Until it ships, scripts and agents
can raise any event with `hotsheet-cli notify send --event <kind>`.

### 32.8.1 Project stopped or stalled (HS2-2MM07G)

The router raises `project.stopped` for each hosted store from ticket **claims**. Every
AI session that follows the Hot Sheet workflow claims the ticket it works on, renews the
lease during long work, and releases it when it stops, whatever its tool or transport.

- **Active.** At least one ticket in the store holds a live claim: a worker and an
  unexpired lease.
- **Stopped (finished).** The last claim was released, and no new claim appeared within
  a 120-second grace. The grace exists so that a worker moving to its next ticket is not
  reported. The message reads "Hot Sheet project _name_ has stopped working on tickets",
  has severity `warning`, and lists the tickets completed during the active period.
- **Stalled (stuck).** A claim's lease expired without renewal, so its session stopped
  renewing: it crashed, hung, or halted. The report waits until no live claim remains in
  the store. It then reads "Hot Sheet project _name_ has stalled", has severity `error`,
  and names each expired claim's ticket and worker.

Each active period is reported once; its dedupe key is the store plus the period's
start. A check runs before each new claim is recorded, so a period whose grace already
ended is reported rather than continued. The tracker is seeded from each store's index
when the router starts. It runs with or without a rule, because it also feeds the live
state (§32.8.3); messages are sent only when an enabled rule names `project.stopped`.
Grace and lease deadlines are one event-driven server timer per store; no request polls.
Sessions that never claim tickets produce no signal.

Two stuck signals cover sessions that still hold claims (HS2-P57XHV):

- **May be stuck (idle).** Claims are held but nothing progressed for the idle window
  (default 30 minutes). Progress is an `activity` or `turn_event` for the store, or a ticket
  write that changes its notes, status, phase, title, or details; creating a ticket is
  such a write (HS2-JTTQMY). A claim renewal is not
  progress, so a session that keeps renewing while doing nothing is still reported. The
  message reads "Hot Sheet project _name_ may be stuck", has severity `warning`, and names
  the held claims. Each idle episode is reported once, and progress re-arms it.
- **Halted.** When a terminal reports a halt (`terminal_halted`, for example a model at
  capacity), the router looks up the terminal's worker id. That is the id its session claims
  with. Every store where that worker holds a live claim gets "Hot Sheet project _name_: an
  AI session halted", with severity `error`, the ticket, the worker, and the halt reason.

Set the windows with `hotsheet-cli notify project-timing [--grace-seconds S]
[--idle-minutes M]` or `PUT /notification-project-timing`. `--idle-minutes 0` disables idle
detection. They are stored as `project_activity` in `notifications.json`. Raw token counts
are not used: per-turn usage records arrive after a turn ends, so they add nothing to
activity and turn events as a liveness signal.

These state words (Active, Stopped, May be stuck, Stalled, Halted) are also the vocabulary of the
glance surfaces (standby, sidebars, and Stats), which add **Waiting on you**; see
[34-glance-surfaces.md](34-glance-surfaces.md) §34.2 (design only, HS2-3AA1N7).

For example, to push to a phone when any project stops:

```sh
hotsheet-cli notify channel add phone --notifier ntfy --set topic=hs-7f3k
hotsheet-cli notify rule add stopped --event project.stopped --channel phone
```

### 32.8.2 Click-through links (HS2-04J8WS)

Routed messages link to their project and ticket in the client's deep-link form (docs/06,
"Project and ticket deep links"): `<origin>/?project=<id-or-path>&ticket=<slug>`, URL-encoded.

- **Origin.** The origin is `link_base` from `notifications.json`, set with
  `hotsheet-cli notify link-base <URL>` or `PUT /notification-link-base`. Use a LAN or
  public origin when notifications are read on another device. Unset, links use the local
  client's origin recorded in `${HOTSHEET_HOME}/client.json`. With neither, messages carry
  no link.
- **Project.** The project is a linked checkout id when one exists. Otherwise it is the
  store root or project path, which the client also resolves.
- **Placement.** Notifiers with a click field (ntfy `Click`, the Slack button, the Teams
  action, the webhook `url`) carry the link there; others append an Open in Hot Sheet link
  to the body. Spoken output drops it.
- A native `hotsheet://` scheme waits for the native app (docs/06).

### 32.8.3 Live state for glance surfaces (HS2-JTTQMY)

The server keeps one live snapshot per hosted store for the glance surfaces designed on
HS2-CT5EWR: standby, the project and grid sidebars, the stats Right now section, and the
away digest. Its state word comes from the same claim tracker as §32.8.1, so a client and a
notification always agree. The tracker therefore runs whether or not a rule names
`project.stopped`; only its messages wait for a rule.

**State word** (`state`), most urgent first:

| Word             | When                                                                                                                                 |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `halted`         | A worker that holds a live claim reported a halt. It clears when that session resumes, progresses on its claim, or loses its claims. |
| `stalled`        | No live claim remains and a lease expired unrenewed. It stays until a new claim starts the next active period.                       |
| `may_be_stuck`   | Claims are held and nothing progressed for the idle window.                                                                          |
| `waiting_on_you` | A Started ticket waits on an unanswered feedback request; review state and tickets in other columns never count (HS2-JGW031).        |
| `active`         | An active period is open, including the release grace.                                                                               |
| `stopped`        | Nothing else applies.                                                                                                                |

`activity.word` carries the tracker's word alone, without Waiting on you.

**Values.** Times are Unix milliseconds, `null` when unknown.

- `activity`: period start, stop time, last progress (the idle detector's definition, so
  claim renewals do not count), the idle deadline, stalled claims, halts, and stuck
  `causes` (§32.8.4).
- `claims`: each live claim's ticket, worker, phase, claim time, lease expiry, and ETA. A
  client computes the ETA overrun locally.
- `workers`: `live` counts distinct workers with a live claim. `expected` comes from the
  coordination worker plan when one exists (HS2-BHPPAS): the unexpired worker sessions registered
  in the `hotsheet-cli coordination` pool of any registered checkout linked to the store whose
  `source_id` is this store (`<git-common-dir>/hotsheet-coordination/state.json`, read on each
  rebuild), united with the live claimants, so a planned worker that never claimed reads as missing.
  Without a plan it counts the workers that held a claim in the current or stalled period. A
  worker that registers without claiming appears at the next rebuild, at most the minute rebuild
  later; clients still never poll.
- `waiting`: the count, plus up to 20 tickets with their reason (`feedback` or `review`).
- Queue runway (the ready Up Next count and an ETA-based estimate) was removed (HS2-5A83MR).
  Clients ignore a `runway` member that an older server still sends.
- `rates.tokens`: tokens per minute averages tokens in plus out over the last 5 whole
  minutes, the current one included. It reads the store's usage JSONL
  ([14](14-metrics-interface.md)) incrementally. Terminal AI sessions record into their
  project's store, not the machine server's bootstrap store, and touch that store's live
  state (HS2-B4EQ87). Claude terminals record each assistant response at its own time as
  it completes, not once at turn end, so a long autonomous turn moves the rate while it
  runs (HS2-SSMDQ1). The snapshot carries the per-minute buckets, so the gauge decays
  without another read. `hours` carries 12 dense UTC-hour token buckets ending with the current hour
  (HS2-TRZGT3), built from the same tail, for the Stats projects sparkline; an older server omits it.
- `rates.tickets_updated`: one time per distinct ticket whose last progress write is within
  60 minutes. A new ticket counts. Claim renewals do not.
- Each rate has a rolling 24-hour peak (`peak_per_minute`, `peak_per_hour`) that its gauge
  scales to (HS2-7CS5A4; it was 30 days). The peak is the maximum over the last 24 whole
  UTC hours, the current one included, so a burst stops setting the scale a day later.
  Peaks are per-hour maxima in the store's gitignored, machine-local
  `metrics/local/live-peaks.json`, which keeps only the period's hours. A book from before
  HS2-7CS5A4 holds per-day maxima and is ignored. The first read backfills the token peak
  from today's and yesterday's usage JSONL.

- `ci`: main CI status (HS2-M8HHEA, §32.8.5), the newest run a linked checkout reported, or
  `status: unavailable` with every other member `null` when none has.

**Delivery.** Each change to a snapshot is pushed as one `live_state` change event whose
`live` member is the whole snapshot. It reaches every client on the existing WebSocket or
long-poll stream and replays by cursor. A rebuild follows a ticket write, activity, a turn,
a usage record, a halt or resume, a terminal prompt opening or closing, or a tracker deadline (lease expiry, grace end, idle
window). The server also rebuilds every hosted store once a minute, even when nothing
happened, so a quiet project's rates visibly decay to zero (HS2-MHW3SX). That is a server
timer; clients still never poll. Rebuilds within 250 ms share one. An unchanged snapshot is
not pushed, and a last-progress move under 15 seconds alone does not push. `epoch_ms`
(server start) and `revision` order snapshots across restarts: a restarted server raises its
epoch above the one its predecessor recorded and continues each store's revision above the
recorded one, so a clock that stepped back cannot reorder them. Clients compute ages and
decaying rates from the timestamps. A display timer may re-render, but it never requests.

**Restarts (HS2-MHW3SX).** The inputs that otherwise live only in server memory are kept per
store in the gitignored, machine-local `metrics/local/live-state.json`, beside the peak book,
and restored best effort when the store is seeded. The record (`version` 1) holds the
tracker's state (claims as last seen, the period start, release grace start, completed
tickets, expired leases, last progress, whether the idle episode was reported, the last
period's outcome, halts with their cause detail, the per-ticket progress times, and the
period's workers), each ticket's progress fingerprint with when it was seen (the newest
5,000), and the last `epoch_ms` and `revision`. A record from before HS2-5A83MR may still hold
`eta_samples`; it loads and they are dropped. It is
rewritten atomically after a rebuild only when its content changed. Terminal prompts are not
kept; they are read again from hook state.

- **Claims.** The index wins. A recorded claim the index no longer holds was released while
  the server was down, so the release grace starts at startup. A recorded claim whose lease
  lapsed meanwhile reads Stalled and is reported once, as if the server had seen it lapse.
- **Tickets updated.** Recorded progress times survive. For tickets the index shows written
  in the last hour, the server also reads the ticket's own record (its latest note, its
  completion, verification, or close time, or its creation) and adds it when it is newer, so
  edits made while the server was down count, and a missing or unreadable record still
  recovers most of the rate. Title edits leave no such trace.
- **First edit after a restart.** The recorded fingerprints are the baseline, so the first
  edit is progress and a renewal is not. For a ticket without one, a content write
  (`created`, `updated`, `closed`, `moved`) is progress, and a claim-only or watcher-observed
  write (`claimed`, `renewed`, `released`, `assigned`, `changed`) is progress only when the
  ticket's own record shows progress in the last 2 minutes. Fingerprints use a stable
  FNV-1a hash so they stay valid across builds.
- An unreadable record or one of another version is ignored, and the store starts as before
  from its index alone. Token rates need no record: they are re-read from the usage JSONL.

**Reads.** `GET /live-state` returns every hosted store, and `GET /stores/{store_id}/live-state`
returns one. Clients read them only when a stream connects, reconnects, or overflows. The
headless form is `hotsheet-cli live-state [--all]`, which prints the selected store's
snapshot (or every hosted store's) from the local server. The tracker lives in the server,
so there is no serverless form.

The web client keeps the snapshots in `clients/web/src/live-state.ts`
(`createLiveStateStore`). Its pure helpers (`tokensPerMinute`, `ticketsUpdatedPerHour`,
`progressAgeMs`, `etaOverrunMs`, `gaugeFraction`, `aggregateLiveStates`, `pausedUntilMs`,
`awaitingInputCauses`) give glance surfaces their numbers. The all-projects gauges scale to the
sum of the stores' peaks, and the workspace grid sidebar's project gauges share that scale
(HS2-VFQM4D).

### 32.8.4 Stuck causes (HS2-EGKKA0)

`activity.causes` says why a worker that holds a live claim in the store is not moving, using
only what the AI tools report. Each cause has `kind`, `worker`, `terminal` (the Hot Sheet
terminal to open, `null` when unknown), and `since_ms`. Usage-limit pauses come first, then
prompts, each oldest first. A cause never changes the state word, so notifications and clients
keep one vocabulary; a glance surface shows the cause in place of the word it refines
([34](34-glance-surfaces.md) §34.2).

**Usage-limit pause** (`usage_limit`). A terminal halt (§32.8.1) whose tool error type is a
usage or rate limit: Claude's `StopFailure` `rate_limit`, or a Codex rollout's
`usage_limit_exceeded` ([05](05-ai-tool-plugins.md), halted sessions). Neither tool sends a
structured reset time, so `resets_at_ms` is read from the halt message when its instant or
time zone can be resolved (HS2-Z28AD3):

| Form                                                  | Example                                       |
| ----------------------------------------------------- | --------------------------------------------- |
| Claude's legacy pipe and Unix time                    | `Claude AI usage limit reached\|1794023400`   |
| An RFC 3339 instant                                   | `try again at 2026-11-07T03:10:00Z`           |
| A relative time after "reset(s) in" or "try again in" | `resets in 2h 15m`, `try again in 45 minutes` |
| A time of day that names UTC or GMT                   | `resets 3pm (UTC)`                            |
| A time of day with an IANA zone                       | `resets 3pm (America/Los_Angeles)`            |
| A time of day in the host's discoverable local zone   | `try again at 3:10 PM`                        |

An unknown zone, an undiscoverable local zone, or a daylight-saving gap or repeated local
hour yields `resets_at_ms: null`: the reset is unknown, never guessed. A
reset more than 5 minutes before the halt or more than 8 days after it is also unknown. The
cause ends with its halt: the session resumes or prompts again, it progresses on its claim,
or it loses its claims. A reset time passing does not end it, because the session still
waits for a new prompt.

**Awaiting input** (`awaiting_input`). The terminal's session waits for the user at a prompt
its hooks reported. `prompt` is `question` for an `AskUserQuestion` the terminal reported
(`POST /terminals/{id}/question`) and not yet answered, and `permission` for a pending
bridged permission request (`POST /permissions/ask`) whose connection is the session id the
terminal's AI session reported. Prompts are not read from terminal output, so a tool without
these hooks reports no prompt cause. The router re-reads the prompts after each
`terminal_question`, `permission_asked`, `permission_resolved`, or `terminal_ai_connection`
event, with one terminal list per event. The cause ends when the question is answered or
withdrawn, the permission is resolved or its hook disconnects, or the session ends. A prompt
belongs to its terminal's worker, so it shows in every store where that worker holds a live
claim, including a claim made after the prompt opened.

### 32.8.5 Main CI status (HS2-M8HHEA)

The server has no CI source of its own and never polls one. The tool that already watches CI
reports each run it learns about: `scripts/ci-gate.mjs status`, an integrator waiting in
`final_testing`, or a user's hook for any CI that can run a command. No inbound exposure or
public URL is needed, and the source is independent of the ticket provider.

- **Report.** `POST /checkouts/{reference}/ci-status` (authenticated like every local route)
  takes `status` (`queued`, `in_progress`, `success`, `failure`, or `cancelled`) and optional
  `branch`, `sha` (7 to 64 hex characters), `run_url` (http or https), `workflow`,
  `started_at_ms`, and `finished_at_ms` (finished runs only, not before the start). The server
  stamps `reported_at_ms`. The headless form is `hotsheet-cli ci report`, with one flag per
  member (`--status`, `--branch`, `--sha`, `--run-url`, `--workflow`, `--started-at-ms`,
  `--finished-at-ms`) and `--checkout <ref>`; without `--checkout` it reports for the checkout
  of the current directory that links the selected store.
- **Order.** One run is kept per checkout id. A report whose run started before the stored run
  is stale, and so is a non-terminal report of the same run (same `sha` and `workflow`) after a
  terminal one; a stale report leaves the stored run unchanged and answers `applied: false`.
  Without both start times the later report wins. The answer also names the stored run and the
  hosted stores linked to the checkout.
- **Live state.** Each store's `ci` is the newest run among its linked checkouts, by start time
  and then report time, with the reporting `checkout`. An applied report rebuilds every linked
  hosted store's snapshot, so it reaches clients as the usual `live_state` event; nothing is
  pushed for a stale one.
- **Persistence.** Runs are kept in the machine-local `<machine home>/ci-status.json`
  (`version` 1) and read again after a restart. An unreadable file or one of another version
  is ignored, and the stores start as `unavailable`.
- **Limit.** Status is only as fresh as the last report. Wiring `ci-gate.mjs` and the
  integrator to report is HS2-W4BC5Q; the Stats tile and the project sidebar chip and row are
  HS2-E16ZEK and HS2-57BYEA.

## 32.9 Headless CLI and REST API

Both call the same `hotsheet_notify::admin` workflow, and every operation is
idempotent.

| CLI (`hotsheet-cli notify …`)                                                                                                                                                | REST                                                         |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ |
| `notifiers [--json]`                                                                                                                                                         | `GET /notifiers` (with `load_errors`)                        |
| `config [--json]`                                                                                                                                                            | `GET /notification-config`                                   |
| `channel add <id> --notifier <n> [--label] [--set k=v]… [--disabled] [--quiet HH:MM-HH:MM [--quiet-zone <IANA> \| --quiet-offset +HH:MM] --quiet-errors] [--max-per-hour n]` | `PUT /notification-channels/{id}`                            |
| `channel secret <id> <field>` (prompt or piped stdin)                                                                                                                        | `PUT /notification-channels/{id}/secrets/{field}`            |
| `channel enable\|disable <id>`                                                                                                                                               | `PUT /notification-channels/{id}` with `enabled`             |
| `channel remove <id>`                                                                                                                                                        | `DELETE /notification-channels/{id}`                         |
| `channel trust <id>` (approve the external plugin as it is now)                                                                                                              | `POST /notification-channels/{id}/trust`                     |
| `channel test <id> [--json]`                                                                                                                                                 | `POST /notification-channels/{id}/test`                      |
| `rule add <id> --event … --channel … [filters incl. --text, --view] [--cooldown s]`                                                                                          | `PUT /notification-rules/{id}`                               |
| `rule remove <id>`                                                                                                                                                           | `DELETE /notification-rules/{id}`                            |
| `send --title … [--body\|--body-file] [--channel] [--event] [--attach]…`                                                                                                     | `POST /notifications` (published events route through rules) |
| `link-base [URL] [--clear]` (show, set, or clear the link origin)                                                                                                            | `PUT /notification-link-base` (`{"url": …}` or `null`)       |
| `project-timing [--grace-seconds S] [--idle-minutes M]`                                                                                                                      | `PUT /notification-project-timing`                           |
| `deliveries [--limit] [--json]`                                                                                                                                              | `GET /notification-deliveries?limit=`                        |

- `--set` values are parsed by the field's type; lists are comma-separated. A secret
  field passed to `--set` is refused, and the error names the `channel secret` command.
- Re-adding a channel with the same notifier keeps the credential references it
  already has.
- Removing a channel also removes rules left without a channel, and deletes the
  `notify-<channel>-<field>` credentials Hot Sheet stored for it.
- `send` and `channel test` exit non-zero when a delivery fails.

### 32.9.1 Web settings (HS2-DPBJJA)

App Settings → **Notifications** (Lucide `bell`) sits with the app categories, because the
configuration is machine-wide. It runs the same REST administration as the CLI. Top to
bottom, it shows:

- **Delivery options.** Link origin, "stopped after" seconds, and "stuck after" minutes,
  each saved on change.
- **Channels.** Each row has the notifier icon, the channel name, the notifier, and a
  Disabled badge. Its menu offers Send Test, Edit…, Enable or Disable, Approve Plugin (for
  an external plugin), and Remove.
- **Rules.** Each row shows the event names and a "To …" summary of channels and filters.
  Its menu offers Edit… and Remove.
- **Recent deliveries.** The last 20 records, with outcome icons and a Refresh action.

**Channel dialog.** Add Channel… and Edit… open a modal dialog whose fields are generated
from the notifier's settings schema:

- text, number, or comma-separated list inputs;
- a switch for a boolean;
- a select for a choice;
- a password input for a credential.

A choice the person makes in the open dialog stays chosen until it is saved or the dialog
closes, even when the dialog re-renders on a resize (HS2-XCZY10).

A credential input never shows a stored value ("Stored; leave empty to keep it"), and it is
written only through the secret route. Quiet hours and the hourly limit sit below. A
**Quiet Time** select offers "No quiet time" (the default for a new channel; saving removes any
window) or "Daily quiet hours", which reveals the window's times and zone, starting at 22:00 to
07:00 for a new window (HS2-WVJNYM).
**Quiet From** and **Quiet Until** are selects of every quarter hour, shown in the device's own
clock format (for example "10:00 PM" where the locale uses a 12-hour clock, "22:00" where it
uses 24 hours) and saved as `HH:MM`; a saved time off that grid, such as one set from the CLI,
stays a choice (HS2-NNC8BG). The quiet-hours **Time Zone** defaults to the device's zone, labeled "Local time zone
(America/New York)", and lists every zone the browser knows; a window saved with only a
fixed offset keeps it as a "Fixed offset" choice until another zone is picked. The CLI's
`--quiet` likewise defaults to the machine's local zone unless `--quiet-zone` or
`--quiet-offset` names one (HS2-9T27WQ).

**Rule dialog.** Choose events and channels with checkboxes; filter by ticket text, tags,
statuses, categories, priorities, and projects, a minimum severity, and a cooldown. **In
Saved Views** lists every open project's saved views ("Project · View"); checking several
matches any of them (HS2-DNQFB8). A view the rule names that no open project offers, such as
one set from the CLI for a closed project, stays listed by its `checkout/view` id, so editing
keeps or removes it deliberately. The rule summary lists categories, priorities, and views.

**Saving and refresh.** Both dialogs submit explicitly, because the server validates the
whole channel or rule. A refusal stays in the dialog with its reason. The category
re-reads from the server when it opens and after each change, and nothing polls.

## 32.10 Built-in notifiers

| Notifier  | Formats                | Limits                                                      | Settings                                                                                                   |
| --------- | ---------------------- | ----------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `speak`   | plain (audio)          | 400-byte utterance; no title field; no attachments          | `voice`, `rate` (wpm), `command` (custom program and arguments), `timeout_seconds`                         |
| `ntfy`    | Markdown, plain        | title 250 B, message 4,096 B; one attachment ≤ 15 MiB       | `server` (default `https://ntfy.sh`), `topic` (required), `token` (secret), `priority`, `tags`             |
| `email`   | HTML + plain, mixed    | subject 200 B, body 200 KB; ≤ 10 attachments, 10 MiB total  | `to` (list, required), `from`, `smtp_host`, `smtp_port` (587), `security`, `username`, `password` (secret) |
| `webhook` | JSON (Markdown body)   | title 500 B, body 64 KB; no attachments                     | `url` (secret, required), `method` (`POST`/`PUT`), `signing_secret` (secret)                               |
| `slack`   | Slack mrkdwn           | header 150 B, section 3,000 characters; no attachments      | `webhook_url` (secret, required)                                                                           |
| `teams`   | Adaptive Card Markdown | title 250 B, body 20 KB; no attachments                     | `webhook_url` (secret, required)                                                                           |
| `twilio`  | plain (SMS)            | 1,600 B; no title field; no attachments (MMS is HS2-K57DK0) | `account_sid`, `auth_token` (secret), `from`, `to` (list), `api_base`                                      |

- **`speak`** runs `say` on macOS, `spd-say --wait` on Linux, and System.Speech through
  PowerShell on Windows, where the text is read from stdin. It never uses a shell, and
  `command` overrides the program.
- **`ntfy`** maps severity to priority when `priority` is `auto`: info and success
  become 3, warning 4, error 5. It also adds severity tags, sends `Click`, and sets
  `Markdown: yes`. With an attachment it sends a PUT with the file as the body and
  RFC 2047-encoded `Message` and `Title` headers. A public topic is readable by anyone
  who knows its name.
- **`email`** uses SMTP through rustls (`starttls`, implicit `tls`, or `none` for a
  local relay only) and sends multipart/alternative plus attachments to every
  recipient.
- **`webhook`** sends the rendered message as JSON: event, severity, title, Markdown body,
  format, url, project, ticket facts, and truncation flags. With a `signing_secret`, it
  adds `X-Hotsheet-Timestamp` and `X-Hotsheet-Signature: sha256=<hex>`, an HMAC-SHA256
  over `<timestamp>.<body>` that a receiver recomputes to verify the sender.
- **`slack`** posts Block Kit to an incoming webhook: a header, a section with the body
  converted to Slack mrkdwn (`*bold*`, `_italic_`, `<url|text>`, `•` bullets, with `&<>`
  escaped), an Open in Hot Sheet button, and a plain `text` fallback.
- **`teams`** posts one Adaptive Card 1.4 to a Teams workflow webhook. The title is
  colored by severity, the body stays Markdown, and the link becomes an Open URL action.
- **`twilio`** sends one SMS per recipient through the Messages API with HTTP basic auth.
  An `MG…` sender is used as a messaging service. If some recipients received the
  message and others failed, the failure is permanent, so a retry never re-texts the
  recipients who already got it.

## 32.11 Privacy and security

- Credentials stay in the OS-backed key registry and appear in configuration only as
  key names. REST never returns them.
- The delivery log records shapes, not content (see Customer Data Hygiene in
  `AGENTS.md`).
- A notification goes only where the user configured it. Ticket titles and blocked
  reasons are part of the message, so a channel to a shared destination shares them.
- Email HTML neutralizes raw HTML from ticket text.

## 32.12 Status and follow-ups

| Item                                                                       | Status  | Ticket     |
| -------------------------------------------------------------------------- | ------- | ---------- |
| Contract, rendering, built-ins, external plugins, rules, router, CLI, REST | Shipped | HS2-R5HTZN |
| Web settings for channels and rules                                        | Shipped | HS2-DPBJJA |
| Webhook, Slack, Teams, SMS notifiers                                       | Shipped | HS2-97ZHMM |
| MMS image attachments through Twilio                                       | Planned | HS2-K57DK0 |
| External plugin digest trust gate                                          | Shipped | HS2-208RB1 |
| Text, checkout, quiet-hour, and rate-limit filters                         | Shipped | HS2-4JG95G |
| Saved-view rules                                                           | Shipped | HS2-G8NXNC |
| Batched summaries of held messages                                         | Shipped | HS2-CFMSP2 |
| Choosing saved views, categories, and priorities in the web rule dialog    | Shipped | HS2-DNQFB8 |
| Click-through links                                                        | Shipped | HS2-04J8WS |
| Configuration write lock                                                   | Shipped | HS2-YGNDET |
| Transitions without a running server (CLI)                                 | Shipped | HS2-8VAXXA |
| Transitions from serverless MCP writes                                     | Shipped | HS2-FYZ5S9 |
| `digest` producer                                                          | Planned | HS2-D33WPH |
| `project.stopped` producer (claims: finished and stalled)                  | Shipped | HS2-2MM07G |
| Idle-while-claimed and halted-session signals; configurable windows        | Shipped | HS2-P57XHV |
| Main CI status in the live state, reported locally                         | Shipped | HS2-M8HHEA |
