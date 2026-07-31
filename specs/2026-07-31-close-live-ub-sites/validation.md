# Validation: Close the Live UB Sites (Phase 2)

## Done when

- [x] `cargo +nightly miri run` completes the demo with no diagnostics, under the
      default Stacked Borrows model.
- [x] The same under `MIRIFLAGS=-Zmiri-tree-borrows`.
- [x] `cargo +nightly miri test` **runs at least one simulation** and passes. This is
      the criterion Phase 0 could not meet: before this phase the only test in the tree
      reads source files, so Miri gated nothing.
- [x] `grep -n unsafe src/pool/mod.rs` returns nothing — Article 1's placement rule,
      not just its soundness rule.
- [x] `grep -rn unsafe src/system/discrete/` returns nothing.
- [x] The `#[expect]` count drops from 9 to 7, and the two removed are exactly the
      `KNOWN UNSOUND` pair. No new `#[allow]` anywhere.
- [x] The disjointness precondition in `pool::buffer` is an `assert!`, not a
      `debug_assert!`, and carries a comment saying why it stays on in release.
- [x] The two `// SAFETY:` comments in `RawSystem::raw_update` have been re-read and
      either still hold or have been corrected to cite the new argument. State which.
- [x] Four gates green: `fmt --check`, `clippy --all-targets -- -D warnings`, `test`,
      `miri test`.
- [x] Trajectory bit-identical to `main`. This is a soundness fix; the numbers must not
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

Tree Borrows on the pre-fix code reported the **same root cause at a different site** —
worth recording, because the two models present it very differently:

```text
error: Undefined Behavior: write access through <33529> at alloc12817[0x0] is forbidden
   --> src/main.rs:38:13          <-- the write inside a user block's `update`
   --> src/main.rs:32:44
   --> src/pool/mod.rs:108:32     <-- same origin as Stacked Borrows reports
```

Stacked Borrows blames the retag in the solver; Tree Borrows blames the eventual write,
which surfaces inside *user block code* that is entirely innocent. The second
presentation is the more alarming one for a block author.

**After the fix: both models clean**, for `miri run` and `miri test`.

### The site-2 question, to be answered honestly

After the pool fix lands, re-run Miri and record which of these happened:

```text
[ ] Miri now reports src/system/discrete/mod.rs — the ptr::read is confirmed.
[x] Miri is clean — and the path IS exercised, roughly 200 sample events in the demo.
[ ] Miri reports something else.
```

**Result: the second site is not undefined behaviour, and the roadmap over-claimed it.**

With the pool site fixed, both borrow models run the demo and the new test suite
completely clean, and the `ptr::read` branch is reached about 200 times per demo run —
so this is not a coverage gap.

Why it is accepted: `ptr::read` copies the reference **bit for bit**, preserving its
borrow tag, so the two handles are not distinguishable to a tag-tracking model. More
importantly the uses are strictly *sequential*, not interleaved — `output_clone` is
moved into `calculate` and never touched again, and the original is used only after
`calculate` returns. The roadmap's claim of "two aliasing `&mut` to the same place"
describes the code's shape but not any execution that actually occurs.

It was still removed, for two reasons that survive the correction:

- Article 1's **placement** rule forbids `unsafe` in block code regardless of soundness.
- Duplicating a `&mut` by bits is fragile with respect to `noalias`: it is safe only
  while nobody uses the original during `calculate`, and nothing enforces that. A
  future edit to those four lines could turn it into real UB with no diagnostic.

So the phase closed **one confirmed UB site and one latent hazard**, which is what the
commit and the roadmap should say rather than "two live UB sites".

### No-behaviour-change check

Same probe-and-trace harness described in the Phase 0 `validation.md`: rebuild
`main.rs`'s model in a throwaway integration test, attach `Probe` blocks returning
`f64::INFINITY`, print at `{:.17e}`, capture on both sides of a `git stash`, diff.

One wrinkle: `DiscreteSystem::calculate`'s signature changed, so a single harness file
cannot compile against both sides. Two variants were used, identical but for the
parameter (`Self::Output<'_>` vs `&mut Self::Output<'_>`) and the write (`*output` vs
`**output`). The model, block logic and numbers are otherwise the same.

**Result: 2016 samples, bit-identical to Phase 0's — exactly matching.**

An additional check, since the doc claim changed: `simulate` on an **empty pool** no
longer panics. Verified with a throwaway test. The `max().unwrap()` that used to panic
disappeared when the input slices moved behind `with_split`, which takes one system's
indices at a time and never needs the maximum arity. The solver-hardening phase's
"handle zero systems" item is satisfied as a side effect.

### Review judgements the gates cannot make

- [x] The `// SAFETY:` comment on the new disjoint-borrow block names the assertion it
      relies on and where that assertion is, rather than restating the operation.
- [x] The comments in `raw_update` cite whatever *actually* establishes non-aliasing
      after this phase. If the argument moved to `pool::buffer` and the comments still
      point at `SystemPool::link`, they are stale — plausible-sounding and wrong, which
      `tech-stack.md` names as the one failure nothing mechanical catches.
- [x] If the fallback was taken and `DiscreteSystem::calculate`'s signature changed,
      `tech-stack.md` records it: Article 6 makes the block-author contract public API.
- [x] If `simulate` gained an allocation per system per step from the closure-shaped
      accessor, that is stated rather than left for someone to discover with a profiler.
      **It did not.** The scratch vector lives in `BufferSet` and is reused; the solver
      keeps one `Vec<usize>` of source indices, also hoisted and reused. The steady
      state allocates nothing per step.
