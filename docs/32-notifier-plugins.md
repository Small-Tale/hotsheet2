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
  - `tag:`, `is:` (`up-next`, `active`, `needs-review`, `open`, `closed`, `duplicate`,
    and the statuses), `has:attachment`, `has:media-annotation`, `attachment:` with `*`
    globs, and `created|updated|completed|verified|started|archived-before|after:` dates.
  - Any other term is a case-insensitive substring of the slug, title, tags, details,
    notes, or attachment names.
  - Differences from the client: a date written without a zone is read as UTC, and
    locale-ordered dates are text. `has:commit` never matches, because it needs a repository
    scan (HS2-VPG8XA). `has:`, `attachment:`, and date terms inside a Boolean expression are
    read structurally here, where the client still reads them as text (HS2-DE9CMP).
- A channel may set `quiet_hours` (`start` and `end` as `HH:MM`, a fixed `utc_offset`
  such as `+09:00`, and `allow_errors`, default true) and `max_per_hour`. Adjust the
  offset when daylight saving time changes.
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
when the router starts, but only if an enabled rule names `project.stopped`. Grace and
lease deadlines are one event-driven server timer per store; no request polls. Sessions
that never claim tickets produce no signal.

Two stuck signals cover sessions that still hold claims (HS2-P57XHV):

- **May be stuck (idle).** Claims are held but nothing progressed for the idle window
  (default 30 minutes). Progress is an `activity` or `turn_event` for the store, or a ticket
  write that changes its notes, status, phase, title, or details. A claim renewal is not
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

## 32.9 Headless CLI and REST API

Both call the same `hotsheet_notify::admin` workflow, and every operation is
idempotent.

| CLI (`hotsheet-cli notify …`)                                                                                                                       | REST                                                         |
| --------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ |
| `notifiers [--json]`                                                                                                                                | `GET /notifiers` (with `load_errors`)                        |
| `config [--json]`                                                                                                                                   | `GET /notification-config`                                   |
| `channel add <id> --notifier <n> [--label] [--set k=v]… [--disabled] [--quiet HH:MM-HH:MM --quiet-offset +HH:MM --quiet-errors] [--max-per-hour n]` | `PUT /notification-channels/{id}`                            |
| `channel secret <id> <field>` (prompt or piped stdin)                                                                                               | `PUT /notification-channels/{id}/secrets/{field}`            |
| `channel enable\|disable <id>`                                                                                                                      | `PUT /notification-channels/{id}` with `enabled`             |
| `channel remove <id>`                                                                                                                               | `DELETE /notification-channels/{id}`                         |
| `channel trust <id>` (approve the external plugin as it is now)                                                                                     | `POST /notification-channels/{id}/trust`                     |
| `channel test <id> [--json]`                                                                                                                        | `POST /notification-channels/{id}/test`                      |
| `rule add <id> --event … --channel … [filters incl. --text, --view] [--cooldown s]`                                                                 | `PUT /notification-rules/{id}`                               |
| `rule remove <id>`                                                                                                                                  | `DELETE /notification-rules/{id}`                            |
| `send --title … [--body\|--body-file] [--channel] [--event] [--attach]…`                                                                            | `POST /notifications` (published events route through rules) |
| `link-base [URL] [--clear]` (show, set, or clear the link origin)                                                                                   | `PUT /notification-link-base` (`{"url": …}` or `null`)       |
| `project-timing [--grace-seconds S] [--idle-minutes M]`                                                                                             | `PUT /notification-project-timing`                           |
| `deliveries [--limit] [--json]`                                                                                                                     | `GET /notification-deliveries?limit=`                        |

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

A credential input never shows a stored value ("Stored; leave empty to keep it"), and it is
written only through the secret route. Quiet hours and the hourly limit sit below.

**Rule dialog.** Choose events and channels with checkboxes; filter by ticket text, tags,
statuses, and projects, a minimum severity, and a cooldown. Filters the dialog does not
show (categories, priorities, and saved views) are kept when a rule is edited, and saved
views appear in the rule summary (HS2-G8NXNC). Choosing them in the dialog is HS2-DNQFB8.

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
| Choosing saved views, categories, and priorities in the web rule dialog    | Planned | HS2-DNQFB8 |
| Click-through links                                                        | Shipped | HS2-04J8WS |
| Configuration write lock                                                   | Shipped | HS2-YGNDET |
| Transitions without a running server (CLI)                                 | Shipped | HS2-8VAXXA |
| Transitions from serverless MCP writes                                     | Shipped | HS2-FYZ5S9 |
| `digest` producer                                                          | Planned | HS2-D33WPH |
| `project.stopped` producer (claims: finished and stalled)                  | Shipped | HS2-2MM07G |
| Idle-while-claimed and halted-session signals; configurable windows        | Shipped | HS2-P57XHV |
