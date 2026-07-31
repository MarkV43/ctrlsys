# Plan: Close the Live UB Sites (Phase 2)

Build order is dictated by one fact: **Miri aborts on the first error**, so the second
site cannot be observed until the first is fixed. Everything is sequenced around
getting a clean read on site 2.

## 1. Pin the baseline before changing anything

- [ ] Record `cargo +nightly miri run` under the default (Stacked Borrows) model.
- [ ] Record it again under `MIRIFLAGS=-Zmiri-tree-borrows`. The roadmap claims the
      pre-`pool/` pattern was accepted under both; this establishes what the *current*
      code does under both, so "fixed" can be claimed against each.
- [ ] Note whether Tree Borrows reports the same site, a different one, or nothing.
      They are different models — a clean Tree Borrows run would not mean the code is
      sound under Stacked Borrows.

## 2. `pool::buffer` — a safe disjoint-borrow API

This is where the phase's design decision lands: the `unsafe` moves out of the solver
and into the module Article 1 designates for buffer allocation, with the aliasing
argument living next to the allocation it depends on.

- [ ] Introduce a `BufferSet` (or equivalent) in `src/pool/buffer.rs` owning the
      `Vec<AlignedBuffer>` that `simulate` currently holds.
- [ ] Give it a **callback-shaped** accessor rather than one returning borrows:

      ```rust
      pub(crate) fn with_split<R>(
          &mut self,
          out: usize,
          inputs: &[usize],
          f: impl FnOnce(&mut [u8], &[&[u8]]) -> R,
      ) -> R
      ```

      The closure shape is deliberate — see `requirements.md`. Returning
      `(&mut [u8], Vec<&[u8]>)` forces a fresh `Vec` per call, because the borrows
      carry a per-call lifetime that a hoisted `Vec<&'x [u8]>` cannot outlive. With a
      closure the borrows never escape, so the scratch storage can live inside
      `BufferSet` and be reused.
- [ ] Assert disjointness inside: `out` must not appear in `inputs`, `inputs` must
      have no duplicates that matter, and every index must be in range. **Always-on
      `assert!`, never `debug_assert!`** — this check is what the unsafe below it
      relies on, and a soundness check compiled out in release is not a check.
- [ ] Write the `// SAFETY:` comment against that assertion: distinct indices are
      distinct `AlignedBuffer`s, each owning a separate allocation, so the `&mut` and
      the `&`s cover disjoint memory.
- [ ] One unsafe operation per block, per the Phase 0 lint block.

## 3. Solver — consume it, and become `unsafe`-free

- [ ] Replace the `output_buffers` local in `simulate` with the new type.
- [ ] Rewrite the per-system body to call `with_split`, passing the link's source
      indices as `inputs` and `idx` as `out`.
- [ ] Delete the raw-pointer derivation (`as_ptr().cast_mut()`, `from_raw_parts_mut`)
      and the `#[expect(clippy::undocumented_unsafe_blocks, …)]` above it.
- [ ] Confirm `src/pool/mod.rs` now contains **no `unsafe` at all** — grep it. That is
      Article 1's placement rule satisfied, not just its soundness rule.
- [ ] Keep `input_ref_buffer`'s allocation reuse. If the closure shape forces an
      allocation per system per step, say so in `validation.md` rather than accepting
      it silently.

## 4. Re-run Miri and find out about site 2

- [ ] `cargo +nightly miri run` again, both borrow models.
- [ ] **Record what actually happens.** The discrete `ptr::read` has never been
      observed failing — it was found by reading. Three outcomes, all reportable:
      Miri now flags it (expected); Miri is clean (the read is reachable but not
      diagnosed, or is not reachable in the demo model); or Miri flags something else
      entirely.
- [ ] If Miri is clean here, do **not** describe the site as "confirmed" anywhere in
      the docs. Fix it on the strength of the reading argument and say that is what
      the evidence is.

## 5. `system::discrete` — stop duplicating the `&mut`

`ptr::read` on `&output` duplicates a mutable reference: the copy goes to `calculate`
while the original stays live for `payload_ref()` and `update_output()`.

- [ ] **Preferred: add a reborrow to `SystemDataOut`.** A GAT
      (`type Reborrowed<'b>` plus `fn reborrow<'b>(&'b mut self) -> Self::Reborrowed<'b>`)
      lets `update` hand `calculate` a reborrow and keep the original. The impl for
      `&'a mut T` is `&mut **self` — **safe code, no unsafe added anywhere**, and
      `DiscreteSystem::calculate`'s signature is untouched, so the block-author
      contract does not change.
- [ ] Only two impls of `SystemDataOut` exist (`()` and `&'a mut T`), so the GAT costs
      little.
- [ ] **Fallback if the bounds do not resolve cleanly:** change `calculate` to take
      `&mut Self::Output<'_>` instead of by value. Also fully safe, but it *is* a
      change to a block-author-facing contract, so Article 6 makes it public API — it
      then needs a note in `tech-stack.md` and a mention in the commit. Try the
      reborrow first; do not burn the phase on type machinery if it fights back.
- [ ] Either way, delete the `#[expect(…, "KNOWN UNSOUND")]` attribute and confirm no
      `unsafe` remains in `src/system/discrete/`.

## 6. Make Miri a real gate

Phase 0 found `cargo +nightly miri test` exercises no simulation at all — the only
test in the tree reads source files. Without this, the fix is hand-verified once via
`miri run` over `src/main.rs` and never checked again.

- [ ] Add an integration test building a small model (a source, a filter, a two-input
      block) and running `simulate` to completion.
- [ ] Gate the length: `#[cfg(miri)]` uses a short `total_time`, the stable build runs
      the full length. `tech-stack.md` already prescribes this.
- [ ] Assert something real about the result, not merely that it did not panic — the
      point is a trajectory that a later regression would change.
- [ ] Verify the test actually executes under `cargo +nightly miri test`.

## 7. Re-check what Phase 0 wrote

- [ ] Re-read the two `// SAFETY:` comments in `RawSystem::raw_update`
      (`src/system/mod.rs`). Both cite `SystemPool::link`'s
      `assert!(!from.ids().contains(&to.id()))` as what establishes non-aliasing. If
      the disjointness argument now lives in `BufferSet` instead, the comments must
      cite *that* — the old citation would be stale, and a stale SAFETY comment is
      exactly what Article 3 says nothing mechanical can catch.
- [ ] Re-read the `# Safety` sections on `from_slices` / `from_slices_mut`. The
      exclusivity clause on `from_slices_mut` should still be true; check rather than
      assume.
- [ ] Confirm the `#[expect]` count dropped from 9 to 7 and that both removals are the
      Phase 2 pair.

## 8. Verify

- [ ] Four gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
      `cargo test`, `cargo +nightly miri test`.
- [ ] `cargo +nightly miri run` clean under **both** borrow models.
- [ ] Trajectory unchanged from `main` — the probe-and-trace method in the Phase 0
      `validation.md`. This is a soundness fix, not a behaviour change; if the numbers
      move, something else happened and it needs explaining.
