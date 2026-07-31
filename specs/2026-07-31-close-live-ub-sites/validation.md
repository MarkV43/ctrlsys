# Validation: Close the Live UB Sites (Phase 2)

## Done when

- [ ] `cargo +nightly miri run` completes the demo with no diagnostics, under the
      default Stacked Borrows model.
- [ ] The same under `MIRIFLAGS=-Zmiri-tree-borrows`.
- [ ] `cargo +nightly miri test` **runs at least one simulation** and passes. This is
      the criterion Phase 0 could not meet: before this phase the only test in the tree
      reads source files, so Miri gated nothing.
- [ ] `grep -n unsafe src/pool/mod.rs` returns nothing — Article 1's placement rule,
      not just its soundness rule.
- [ ] `grep -rn unsafe src/system/discrete/` returns nothing.
- [ ] The `#[expect]` count drops from 9 to 7, and the two removed are exactly the
      `KNOWN UNSOUND` pair. No new `#[allow]` anywhere.
- [ ] The disjointness precondition in `pool::buffer` is an `assert!`, not a
      `debug_assert!`, and carries a comment saying why it stays on in release.
- [ ] The two `// SAFETY:` comments in `RawSystem::raw_update` have been re-read and
      either still hold or have been corrected to cite the new argument. State which.
- [ ] Four gates green: `fmt --check`, `clippy --all-targets -- -D warnings`, `test`,
      `miri test`.
- [ ] Trajectory bit-identical to `main`. This is a soundness fix; the numbers must not
      move.

## How to check

### The four gates

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo +nightly miri test
```

All four green. Unlike Phase 0, there is no expected-red gate this time — closing that
gap is the phase's point.

### Both aliasing models

```bash
cargo +nightly miri run
```

```bash
MIRIFLAGS=-Zmiri-tree-borrows cargo +nightly miri run
```

Run both **before** touching anything, to pin the baseline, and after. Stacked and Tree
Borrows are different models: passing one is not evidence about the other.

### Baseline to beat

Recorded 2026-07-30 on `main` at `a413e05`, default Stacked Borrows:

```text
error: Undefined Behavior: trying to retag from <33736> for Unique permission at
       alloc12817[0x0], but that tag only grants SharedReadOnly permission for this location
   --> src/pool/mod.rs:122:45
help: <33736> was created by a SharedReadOnly retag at offsets [0x0..0x8]
   --> src/pool/mod.rs:108:32
error: aborting due to 1 previous error
```

Tree Borrows result on the current code: **not yet measured** — that is task group 1.

### The site-2 question, to be answered honestly

After the pool fix lands, re-run Miri and record which of these happened:

```text
[ ] Miri now reports src/system/discrete/mod.rs — the ptr::read is confirmed.
[ ] Miri is clean — the read is real by inspection but not exercised by the demo model.
[ ] Miri reports something else — investigate before proceeding.
```

Fill this in with the actual result. The site was found by reading, not by a
diagnostic, and the docs must not call it confirmed unless it is. If it stays
unconfirmed, consider whether the new Miri-runnable test can be shaped to exercise a
held discrete block and settle it.

### No-behaviour-change check

Same probe-and-trace harness described in the Phase 0 `validation.md`: rebuild
`main.rs`'s model in a throwaway integration test, attach `Probe` blocks returning
`f64::INFINITY`, print at `{:.17e}`, capture on both sides of a `git stash`, diff.

Phase 0's result was 2016 samples bit-identical. This phase should match that exactly —
same model, same numbers. A diff means the fix changed evaluation order or timing, not
just aliasing, and needs explaining before merge.

### Review judgements the gates cannot make

- [ ] The `// SAFETY:` comment on the new disjoint-borrow block names the assertion it
      relies on and where that assertion is, rather than restating the operation.
- [ ] The comments in `raw_update` cite whatever *actually* establishes non-aliasing
      after this phase. If the argument moved to `pool::buffer` and the comments still
      point at `SystemPool::link`, they are stale — plausible-sounding and wrong, which
      `tech-stack.md` names as the one failure nothing mechanical catches.
- [ ] If the fallback was taken and `DiscreteSystem::calculate`'s signature changed,
      `tech-stack.md` records it: Article 6 makes the block-author contract public API.
- [ ] If `simulate` gained an allocation per system per step from the closure-shaped
      accessor, that is stated rather than left for someone to discover with a profiler.
