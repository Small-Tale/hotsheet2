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
exit status. A failed first push removes only the `origin` that attempt just added, so
correcting the URL or credentials and choosing **Connect & push** again is safe.

Jira is an issue provider rather than a Git host. Connect Jira from **Settings → Ticket sources** when Jira itself should own the tickets; use a Git remote only for Hot Sheet Git ticket repositories.
