## Hot Sheet — ticket workflow

This project tracks work as Hot Sheet tickets in a linked git-backed store.

- Read priority work with `hotsheet-cli ls --up-next` and `hotsheet-cli show <slug>`.
- Mark work started, then completed with a note using `hotsheet-cli edit`.
- Use the `hotsheet_*` MCP tools when available; they use the same serverless engine.
- Create follow-up tickets for unfinished work rather than leaving only TODO comments.

**Write portable durable references.** In documentation, ticket text, and AI-authored notes,
do not copy a developer-specific home directory, username, Desktop/Documents path, or absolute
clone location. Use repository-relative paths in the current project. For another repository,
use its stable name and canonical URL when helpful, or a placeholder such as `<repo-root>/path`.
Keep an exact local path only when it is indispensable machine-local diagnostic evidence, and
label it as local context rather than shared project structure.
