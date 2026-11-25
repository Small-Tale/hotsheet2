# Back up a Hot Sheet ticket repository

Hot Sheet ticket repositories are Git repositories. Connecting one to a private remote keeps ticket history recoverable and lets your other devices synchronize it.

1. Create an empty private repository on [GitHub](https://docs.github.com/en/repositories/creating-and-managing-repositories/creating-a-new-repository), [GitLab](https://docs.gitlab.com/user/project/), or another Git host. Do not initialize it with a README or license.
2. Copy its clone URL. SSH URLs such as `git@github.com:you/project-tickets.git` are convenient when your SSH key is already configured; HTTPS clone URLs also work.
3. Paste that URL into Hot Sheet’s **Remote URL** field and choose **Connect & push**.

If authentication fails, configure an SSH key or credential helper using your Git host’s instructions, then try again. Hot Sheet never asks for or stores your Git-host password in the ticket repository.

When Git rejects the connection or first push, Hot Sheet keeps Git's diagnostic text
and adds next steps for common cases: an existing `origin`, a missing or inaccessible
repository, SSH/token authentication, SSH host-key trust, network/DNS failures, a
non-empty remote, or a repository with no commit yet. An unrecognized Git failure is
shown with its operation context and original diagnostic instead of being reduced to an
exit status. Hot Sheet saves the entered URL as `origin` before pushing and keeps it when
the push or a later check fails, and the error says so (HS2-8SEWHJ). Choosing
**Connect & push** again with the same URL reuses it. A corrected URL replaces an origin
Hot Sheet saved (recorded as `hotsheet.backupOrigin` in the store's Git config), while an
`origin` you configured yourself is never replaced.
New stores receive an initial committed schema baseline before this screen is shown, so
the first push never depends on a later ticket mutation creating `HEAD`.

Jira is an issue provider rather than a Git host. Connect Jira from **Settings → Ticket sources** when Jira itself should own the tickets; use a Git remote only for Hot Sheet Git ticket repositories.

For an imported HS1 repository, the first backup runs in a project-owned background
job. The setup dialog closes after acceptance; the banner and project tab show Git's
reported object counts and byte/rate values where available. Other projects remain
usable, and reload restores the running or failed attempt. Waiting for the remote and
verifying the intended import revision remain indeterminate phases. Retry retains
its owning store and remote. The remote is saved as `origin` before the readiness check
and push, so a failure keeps it, and Change backup replaces it with a corrected URL.
Successful push is followed by authoritative backup verification before the
old-source cleanup action becomes available. See [migration ownership and progress](07-migration.md#background-ownership-and-progress-protocol).
