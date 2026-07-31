# Requirements: Close the Live UB Sites (Phase 2)

## Scope

**In:**

1. `src/pool/mod.rs` — the Miri-confirmed SharedReadOnly→Unique retag.
2. `src/system/discrete/mod.rs` — the `ptr::read` that duplicates a `&mut`. (Turned out
   **not** to be UB; removed anyway. See the Context section.)
3. A simulation test that actually runs under `cargo +nightly miri test`.
4. Re-checking the `// SAFETY:` comments and `# Safety` contracts Phase 0 wrote, since
   the pool fix changes what establishes non-aliasing.

**Out:**

- `HeldSystem`'s `self.last_time + 2.0 * req_dt` next-event calculation. It sits in the
  same function being rewritten and is tempting to "fix while we're here", but it is a
  *timing* question, not a soundness one, and belongs to the discrete-hardening phase.
  Changing it here would move numbers in a phase whose whole claim is that numbers do
  not move.
- The other solver-robustness items — Phase 3. (The empty-pool `max().unwrap()` panic
  went away by itself: the reduction it lived in disappeared with the rewrite. Verified,
  and noted in `validation.md`, rather than claimed.)
- Anything about `output_ref_buffer` being hard-coded to one slice; mux outputs are a
  stated non-goal.

## Decisions

### The `unsafe` moves to `pool::buffer`; the solver ends up with none

This is the decision that shapes the phase, and it comes from a conflict the roadmap
does not mention.

Article 1 has two clauses. The famous one is "no safe function may cause undefined
behaviour". The second is a *placement* rule: "`unsafe` appears only in designated
modules (signal layout and buffer allocation). It must not appear in solver, graph, or
block code." `src/pool/mod.rs` is the solver and `src/system/discrete/` is a block
combinator — **both sites violate the placement rule as well as the soundness rule.**

The roadmap's prescribed fix — derive the raw pointer from a `&mut` once before the
loop and take both borrow kinds from it — fixes the soundness and leaves the placement
violation exactly where it is. Taking it would mean amending Article 1 or waiving it
silently.

So instead the disjoint borrow becomes an operation on the buffer collection itself, in
`src/pool/buffer.rs`, which is a designated module. The solver asks for "one buffer
mutably, these others immutably" and gets safe references back. Three things follow:

- The solver contains no `unsafe`, so Article 1 holds in letter and spirit.
- The aliasing argument lives next to the allocation it depends on, rather than three
  modules away from it.
- The disjointness precondition becomes a checked, named thing instead of a comment
  asserting that `SystemPool::link` made it true somewhere else.

### The accessor takes a closure, not a return value

`fn split(&mut self, …) -> (&mut [u8], Vec<&[u8]>)` reads better but forces an
allocation per system per step. The returned borrows carry a lifetime tied to that one
`&mut self`, and `simulate` currently hoists `input_ref_buffer` out of the loop to
reuse its allocation — a hoisted `Vec<&'x [u8]>` cannot hold borrows whose lifetime is
shorter than its own, and `Vec` is covariant, so no amount of variance juggling
recovers it.

A callback (`with_split(out, inputs, |output, inputs| …)`) keeps the borrows from
escaping, which lets the scratch storage live inside the buffer type and be reused
across calls. The ergonomic cost is one level of nesting in `simulate`.

### The disjointness check is `assert!`, and Article 4 does not quite cover why

Article 4 says `assert!` for user contracts, `debug_assert!` for internal invariants.
The disjointness of `out` and `inputs` is an *internal* invariant — `SystemPool::link`
maintains it — which by a literal reading permits `debug_assert!`.

It gets `assert!` anyway, because it is the precondition of an `unsafe` block. A
soundness check compiled out in release is not a check; it converts a panic into
undefined behaviour in exactly the build where that is least diagnosable. The rule this
phase applies, and which is worth folding back into Article 4: **a check that an
`unsafe` block relies on is always on, regardless of who is capable of violating it.**

### The discrete fix: reborrow first, signature change as fallback

`update` needs `output` to survive being handed to `calculate`. The duplication exists
because `Sys::Output<'_>` is an opaque associated type with no way to reborrow it
generically — that is the actual root cause, not carelessness.

Preferred fix is to give `SystemDataOut` that capability: a GAT `Reborrowed<'b>` and a
`reborrow(&'b mut self)` method. For `&'a mut T` the body is `&mut **self` — safe code,
adding no `unsafe` anywhere, and leaving `DiscreteSystem::calculate`'s signature alone
so the block-author contract is unchanged. Only two impls of `SystemDataOut` exist, so
the cost is small.

**Outcome: the bounds did fight back, and the fallback was taken.** The GAT itself
compiled — trait, both impls, all fine. It broke at the use site: relating the reborrow
to the concrete output type requires
`for<'b> Sys::Output<'s>: SystemDataOut<'s, Reborrowed<'b> = Sys::Output<'b>>`, which
the compiler cannot reconcile with the pre-existing
`for<'b> Sys::Output<'b>: SystemDataOut<'b>` bound — it reports both clauses as
candidate impls and gives up with six inference errors.

So `calculate` takes `&mut Self::Output<'_>`. Equally sound and equally free of
`unsafe`, at the cost of a block-author-facing contract change: `**output = value`
where it was `*output = value`. Article 6 makes that public API, so it is recorded in
`tech-stack.md`. The GAT was reverted rather than left in place unused.

## Context

### The second site was not undefined behaviour — the roadmap over-claimed

This was written as an open question, and it now has an answer worth recording plainly.

With the pool site fixed, both borrow models run the demo and the whole test suite
clean. The `ptr::read` branch executes roughly 200 times per demo run, so this is not a
coverage gap — Miri simply does not consider it a violation.

The reason is that the roadmap's argument describes the code's *shape* rather than any
execution. `ptr::read` copies the reference bit for bit, so both handles carry the same
borrow tag and a tag-tracking model cannot tell them apart. More decisively, the uses
are **sequential**: `output_clone` is moved into `calculate` and never touched again,
and the original is used only after `calculate` returns. There is no moment where two
`&mut` are used in an interleaved way.

It was still removed, on two grounds that survive the correction:

- Article 1's placement rule forbids `unsafe` in block code whether or not it is sound.
- A `&mut` duplicated by bits is fragile with respect to `noalias`. It is fine only
  while nobody uses the original during `calculate` — an invariant nothing enforces,
  which a future edit to those four lines could break with no diagnostic to catch it.

So the phase closed **one confirmed UB site and one latent hazard**. The roadmap's
"two live UB sites" should be read as one live and one latent.

### Both borrow models, because they are different claims

Stacked Borrows and Tree Borrows are distinct aliasing models, and Miri's default is
Stacked. The roadmap notes the pre-`pool/` pattern was accepted under both, which is
the standard worth meeting: `-Zmiri-tree-borrows` gets run alongside the default, both
before and after. A clean run under one says nothing about the other.

### The `#[expect]` markers are the phase's proof of landing

Phase 0 left `#[expect(clippy::undocumented_unsafe_blocks, reason = "KNOWN UNSOUND …")]`
on both sites, deliberately, because no true `// SAFETY:` comment existed for unsound
code. Those attributes are this phase's acceptance criterion in mechanical form: if
`unsafe` survives at either site it now needs a real comment, and if it does not, the
attribute must go or clippy will report the expectation as unfulfilled. The `#[expect]`
count should fall from 9 to 7, and the two removed must be exactly this pair.

### Phase 0's SAFETY comments may go stale, which is the failure mode Article 3 names

The two comments in `RawSystem::raw_update` currently cite `SystemPool::link`'s
self-feed assertion as what establishes that no buffer is borrowed shared and unique in
the same step. If that argument moves into the buffer module, those comments become
citations of the wrong thing — still plausible-sounding, still passing every lint.

`tech-stack.md` is explicit that nothing mechanical can catch a false SAFETY comment
and that it stays a review judgement. This phase is the first occasion to exercise that
judgement, so re-reading them is scoped work rather than a nicety.

### Why this is the phase that must not be rushed

`mission.md` ranks a safe API that can trigger UB above incorrect results — the
highest-severity class in the repository. Every later phase's correctness argument is
stated in terms of the four gates, and Miri is the gate that has so far never run a
simulation. Until this phase lands, the crate has a documented, reproducible soundness
bug sitting under a `# Safety` contract that Phase 0 wrote in good faith.
