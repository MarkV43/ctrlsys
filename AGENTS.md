# Agent instructions

## Never commit without explicit permission

Do not run `git commit`. Do not run `git push`. Not at the end of a task, not when the
gates go green, not when the work is obviously finished.

Do the work, run the gates, report what changed, and **stop with the tree dirty**. Say
that it is dirty. Wait to be asked.

- **Permission is per-request, never standing.** "Commit the specs, then begin
  implementation" authorises committing the specs. It does not authorise committing the
  implementation afterwards.
- **Finishing is not permission.** A request to "begin implementation", "fix that", or
  "start the next phase" authorises the *work*. Recording it in history is a separate
  decision, and it is not the agent's.
- **Pushing is a further step beyond committing.** Never fold a push into a commit
  request, and never carry a previous turn's push authorisation forward to a later
  change.
- **Invoking a skill does not grant it.** The `feature-spec` flow, for instance,
  produces a branch and three spec files; committing them is still a separate ask.

## Orientation

`specs/` is the source of truth for intent: `mission.md` (the articles all work is
measured against), `tech-stack.md` (settled architecture and the four verification
gates), `roadmap.md` (phases, in order). Read them before changing code.

Every phase must leave all four gates green:

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo +nightly miri test
```

Run Miri under both aliasing models — they are different claims:

```bash
MIRIFLAGS=-Zmiri-tree-borrows cargo +nightly miri test
```
