# Git: publishing the project on GitHub and living with it

A short practical guide. All commands run in the project directory (`C:\dev\rcsconv`
or wherever it lives). Replace `<nick>` and `<repo>` with your values.

## 1. One-time setup

```powershell
git --version                       # is git installed?
git config --global user.name  "Ivan Rodionov"
git config --global user.email "your@email"     # the same as on GitHub
git config --global init.defaultBranch main
```

If you do not want to publish your personal e-mail: on GitHub go to
`Settings -> Emails -> Keep my email addresses private` and use an address like
`<id>+<nick>@users.noreply.github.com` in git.

Already configured in the repository: `.gitignore` (keeps out `golden/`, `out/`,
`dumps/`, `docs-rus/`, `usernotes.txt`, `rust/target/`), `.gitattributes` (line
endings and binary files), branch `main`.

## 2. Create a repository on GitHub

1. `github.com -> New repository`.
2. Name: `<repo>` (for example `rcsconv`), visibility: Public, do **not** add a
   license/README/.gitignore — they already exist in the project.
3. Copy the address `https://github.com/<nick>/<repo>.git`.

## 3. Connect and push

```powershell
git remote add origin https://github.com/<nick>/<repo>.git
git remote -v                       # check that origin was added
git branch -M main                  # the branch is called main
git push -u origin main
```

**A password will not work.** GitHub asks for a *Personal Access Token*:
`Settings -> Developer settings -> Personal access tokens -> Fine-grained tokens ->
Generate new token`, with `Contents: Read and write` on this repository. Paste the
token into the "Password" prompt. Simpler: install `Git Credential Manager` once (it
ships with Git for Windows) and the token is remembered.

## 4. Daily cycle

```powershell
git status                 # what changed (safe, changes nothing)
git diff                   # what exactly changed, line by line
git add -A                 # stage all changes
git commit -m "what was done" # record a snapshot
git push                   # send to GitHub
```

Useful:

| Command | Why |
|---|---|
| `git log --oneline -10` | the last 10 commits |
| `git diff --stat` | which files changed and by how much |
| `git show --stat HEAD` | what went into the latest commit |
| `git status --short` | compact view |

## 5. If something went wrong

```powershell
git restore <file>          # discard unsaved edits of a file
git restore --staged <file> # unstage a file (edits stay)
git reset --soft HEAD~1     # undo the last commit, changes stay
git commit --amend -m "..." # fix the last commit message (before push)
```

"Irreversible" applies only to `git reset --hard` and `git push --force`; without
them everything is recoverable. If in doubt, run `git status` and `git log` first.

## 6. What not to commit

- `golden/` (Autodesk data and reference files), `out/` (results), `dumps/`;
- the Russian documentation mirror `docs-rus/` (local only by design);
- personal notes (`usernotes.txt` is already in `.gitignore`);
- GitHub rejects files larger than 100 MB — our data is exactly that size.

Check before pushing: `git status --short` must not list anything from `golden/` or
`out/`. If a file accidentally got into the index but not into the commit:
`git restore --staged <file>`.

## 7. Branches (when needed)

```powershell
git switch -c feature/name     # a new branch from the current one
git switch main                # go back
git merge feature/name         # merge into main (run from main)
```

## 8. What the repository already contains for GitHub

- `README.md` — the shop window (what it does, how it was verified, how it was made);
- `LICENSE` (MIT), `pyproject.toml` (metadata), `.gitattributes`;
- `.github/workflows/tests.yml` — after the first push GitHub runs 85 Python tests on
  3.11/3.12 and `cargo test --workspace` on stable Rust, and shows a tick;
- badges for the README (add after creating the repository):
  `![tests](https://github.com/<nick>/<repo>/actions/workflows/tests.yml/badge.svg)`
  and `![license](https://img.shields.io/badge/license-MIT-blue)`.
