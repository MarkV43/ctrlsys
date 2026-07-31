//! The signal interface: how a block's typed inputs and outputs are built from the
//! raw byte slices the solver hands it.
//!
//! # The one-slice-per-link design
//!
//! A composite input such as `(&'a f64, &'a f64)` is **never laid out in memory as a
//! tuple**. It is built from two independent slices, each borrowing a different
//! producer's output buffer. Nothing in this module depends on the layout of a
//! composite type: only *leaf* types are ever cast from bytes, and each leaf is cast
//! from its own slice.
//!
//! That is what makes the `unsafe` here tractable. There are no computed offsets that
//! could disagree with a type, no padding questions, and no copies — every input is a
//! borrow of the producer's output. The obligations that remain are stated on each
//! declaration below under `# Safety`, and they reduce to four things per slice:
//! count, length, alignment, and validity.

pub mod discrete;

use std::marker::PhantomData;

pub trait System<'s> {
    type Input<'a>: SystemDataIn<'a>;
    type Output<'a>: SystemDataOut<'a>;

    fn update(&mut self, time: f64, input: Self::Input<'s>, output: Self::Output<'s>) -> f64;
    /// `true` if system has internal state / delay that makes it break algebraic loops.
    fn is_stateful(&self) -> bool {
        false
    }
}

pub trait RawSystem {
    fn raw_update<'a>(&mut self, time: f64, input: &[&'a [u8]], output: &mut [&'a mut [u8]])
    -> f64;
    fn is_stateful(&self) -> bool;
    fn output_alignment(&self) -> usize;
    fn output_size(&self) -> usize;
}

impl<Sys> RawSystem for Sys
where
    for<'s> Sys: System<'s>,
{
    fn is_stateful(&self) -> bool {
        System::is_stateful(self)
    }

    fn output_alignment(&self) -> usize {
        align_of::<Sys::Output<'_>>()
    }

    fn output_size(&self) -> usize {
        size_of::<Sys::Output<'_>>()
    }

    fn raw_update<'a>(
        &mut self,
        time: f64,
        input: &[&'a [u8]],
        output: &mut [&'a mut [u8]],
    ) -> f64 {
        // SAFETY: `SystemDataIn::from_slices` requires one slice per link, each of at
        // least the leaf's size, aligned to the leaf's alignment, holding a valid
        // value, and immutable for `'a`. Length, alignment and validity are
        // established by the caller's allocator: `BufferSet` gives every system an
        // `AlignedBuffer` sized by `output_size()` and aligned by
        // `output_alignment()`, zero-initialised, and each input slice borrows one such
        // buffer whole. Immutability for `'a` is established by
        // `BufferSet::with_split`, which hands out these slices as ordinary shared
        // references and asserts that none of them is the buffer borrowed mutably for
        // `output`. The slice *count* matching `Sys::Input`'s arity is the caller's
        // obligation and is checked inside the impl.
        let i_ref = unsafe { Sys::Input::from_slices(input) };
        // SAFETY: `SystemDataOut::from_slices_mut` requires the same four properties
        // plus exclusivity. Size, alignment and validity come from the same
        // `AlignedBuffer` as above. Exclusivity is the borrow checker's: `output`
        // reaches this call as a genuine `&mut` handed out by `BufferSet::with_split`,
        // which took it from `&mut self.buffers[out]` after the input borrows were
        // taken, having asserted `out` is not among them.
        let o_ref = unsafe { Sys::Output::from_slices_mut(output) };

        Sys::update(self, time, i_ref, o_ref)
    }
}

pub trait SystemDataIn<'a> {
    type Payload: ?Sized;

    /// Builds this input view by casting one byte slice per link.
    ///
    /// Every implementation fixes an **arity** `N`: the number of leaf references the
    /// type is composed of — zero for `()`, one for `&T`, two for `(&T, &U)`. Write the
    /// leaf types as `L_0 … L_{N-1}`. Only these leaves are cast; no composite value is
    /// reconstructed from bytes.
    ///
    /// # Safety
    ///
    /// The caller must guarantee all of the following:
    ///
    /// - `slices.len() == N`, the arity of `Self`. This is the one obligation the type
    ///   system cannot express, because it relates a *graph* property — how many links
    ///   were attached to this system — to a *type* property, the shape of `Self`.
    ///   See `specs/mission.md` Article 2.
    /// - For every `i`, `slices[i].len() >= size_of::<L_i>()`.
    /// - For every `i`, `slices[i].as_ptr()` is aligned to `align_of::<L_i>()`.
    /// - For every `i`, the first `size_of::<L_i>()` bytes of `slices[i]` hold a valid,
    ///   fully initialised value of `L_i`.
    /// - For every `i`, the referenced memory stays allocated and is **not mutated**
    ///   for the whole of `'a`. The returned value contains shared references; another
    ///   live `&mut` to the same bytes is undefined behaviour.
    ///
    /// Within this crate the last four hold by construction. Every slice borrows a
    /// producer's `AlignedBuffer`, allocated at exactly the size and alignment that
    /// producer reports through `RawSystem::output_size` and
    /// `RawSystem::output_alignment`, so length and alignment cannot disagree with the
    /// type. Validity holds because the buffer is zero-initialised and every leaf type
    /// used as a signal has a valid all-zero bit pattern. Non-mutation holds because
    /// `BufferSet::with_split` produces these as ordinary shared references and
    /// refuses to also hand out a mutable borrow of any of them.
    ///
    /// Arity is the obligation that remains genuinely open, which is why it is also
    /// checked at run time inside each implementation.
    unsafe fn from_slices(slices: &[&'a [u8]]) -> Self;
}

pub trait SystemDataOut<'a> {
    type Payload: ?Sized;

    /// Builds this output view by casting one mutable byte slice per output port.
    ///
    /// The arity convention is the one described on [`SystemDataIn::from_slices`]. In
    /// practice `N` is 1 today: muxed *outputs* are a stated non-goal, so
    /// `SystemPool::simulate` passes a single slice.
    ///
    /// # Safety
    ///
    /// The caller must guarantee everything [`SystemDataIn::from_slices`] requires —
    /// count, length, alignment, validity — and additionally:
    ///
    /// - **Exclusivity.** For every `i`, no other reference to the bytes of
    ///   `slices[i]`, shared or unique, may be live at any point during `'a`. The
    ///   returned value is the only handle to that memory.
    ///
    /// Validity matters here for a reason particular to this crate rather than to
    /// Rust: `output` is **in-out**. It persists across steps and a block may read its
    /// previous value as state (`specs/mission.md` Article 6), so the bytes are read,
    /// not merely written. The all-zero bit pattern produced by `AlignedBuffer` must
    /// therefore be a valid `L_i`, which is what makes zero-initialisation a sound
    /// starting state rather than merely a convenient one.
    ///
    /// Within this crate exclusivity is the borrow checker's: `BufferSet::with_split`
    /// obtains this slice from `&mut self.buffers[out]` — a real unique borrow, not a
    /// pointer cast — after asserting that `out` is not among the indices borrowed as
    /// inputs. `SystemPool::link` is what makes that assertion hold in practice, by
    /// rejecting a system wired to itself.
    unsafe fn from_slices_mut(slices: &mut [&'a mut [u8]]) -> Self;
    fn copy_from_payload(&mut self, payload: &Self::Payload);
    fn payload_ref(&self) -> &Self::Payload;
}

impl<'a> SystemDataIn<'a> for () {
    type Payload = ();

    /// # Safety
    ///
    /// Vacuous: the arity of `()` is zero, so no slice is read, no pointer is formed
    /// and no memory is dereferenced. `slices` is ignored entirely — a caller may pass
    /// any slice, including an empty one, without risk. This impl exists so that a
    /// source block with no inputs satisfies the same trait as every other block.
    unsafe fn from_slices(_: &[&'a [u8]]) -> Self {}
}

impl<'a> SystemDataOut<'a> for () {
    type Payload = PhantomData<()>; // TODO replace with ! when stabilized

    /// # Safety
    ///
    /// Vacuous, for the reason given on `SystemDataIn for ()`: arity zero, so nothing
    /// is read or written. Used by blocks that exist for their side effects rather
    /// than for a signal.
    unsafe fn from_slices_mut(_: &mut [&'a mut [u8]]) -> Self {}
    fn copy_from_payload(&mut self, _: &Self::Payload) {}

    fn payload_ref(&self) -> &Self::Payload {
        &PhantomData
    }
}

impl<'a, T> SystemDataIn<'a> for &'a T {
    type Payload = T;

    /// # Safety
    ///
    /// Arity one, leaf type `T`. The caller must guarantee that:
    ///
    /// - `slices` has exactly one element (a `debug_assert!` catches an empty `slices`
    ///   in debug builds; it is not a substitute for the contract);
    /// - `slices[0].len() >= size_of::<T>()`;
    /// - `slices[0].as_ptr()` is aligned to `align_of::<T>()`;
    /// - those bytes hold a valid, initialised `T`;
    /// - no `&mut` to those bytes exists for the whole of `'a`.
    ///
    /// See [`SystemDataIn::from_slices`] for where each of these is established inside
    /// this crate.
    unsafe fn from_slices(slices: &[&'a [u8]]) -> Self {
        debug_assert!(
            !slices.is_empty(),
            "System expected 1 input, found {}",
            slices.len(),
        );

        let ptr = slices[0].as_ptr().cast::<T>();
        // SAFETY: the caller guarantees `slices[0]` is at least `size_of::<T>()` bytes,
        // aligned to `align_of::<T>()`, holds a valid `T`, and is not mutably borrowed
        // for `'a` — the four obligations listed above. The returned reference borrows
        // the producer's buffer, which outlives `'a` because `simulate` owns the
        // buffers for the whole run.
        unsafe { &*ptr }
    }
}

impl<'a, T: Clone> SystemDataOut<'a> for &'a mut T {
    type Payload = T;

    /// # Safety
    ///
    /// Arity one, leaf type `T`. The caller must guarantee that:
    ///
    /// - `slices` has exactly one element;
    /// - `slices[0].len() >= size_of::<T>()`;
    /// - `slices[0].as_mut_ptr()` is aligned to `align_of::<T>()`;
    /// - those bytes hold a valid, initialised `T` — this is read, not just written,
    ///   because `output` is in-out state;
    /// - **no other reference of any kind** to those bytes is live during `'a`.
    ///
    /// See [`SystemDataOut::from_slices_mut`] for where each of these is established
    /// inside this crate.
    unsafe fn from_slices_mut(slices: &mut [&'a mut [u8]]) -> Self {
        debug_assert!(!slices.is_empty());

        let ptr = slices[0].as_mut_ptr().cast::<T>();
        // SAFETY: the caller guarantees `slices[0]` is at least `size_of::<T>()` bytes,
        // aligned to `align_of::<T>()`, holds a valid `T`, and is exclusively borrowed
        // for `'a`. Exclusivity is what makes the `&mut` sound, and it comes from
        // `SystemPool::link` refusing to wire a system to itself.
        unsafe { &mut *ptr }
    }

    fn copy_from_payload(&mut self, payload: &Self::Payload) {
        **self = payload.clone();
    }

    fn payload_ref(&self) -> &Self::Payload {
        self
    }
}

impl<'a, T, U> SystemDataIn<'a> for (&'a T, &'a U) {
    type Payload = (T, U);

    /// # Safety
    ///
    /// Arity two, leaf types `T` and `U`. The caller must guarantee, **independently
    /// for each of `slices[0]` and `slices[1]`**, everything the single-leaf impl
    /// requires: sufficient length, correct alignment for that leaf, a valid value,
    /// and no live `&mut` for `'a`. `slices` must have exactly two elements.
    ///
    /// Note what is *absent* from this list: nothing is required of the layout of
    /// `(T, U)`. The tuple is assembled on the stack from two references that were
    /// each cast from a separate producer's buffer, so tuple layout — which Rust
    /// leaves unspecified — never enters the safety argument. This is the property
    /// that the one-slice-per-link design exists to obtain, and the reason a
    /// `zerocopy`-style bound on the composite type is unnecessary.
    unsafe fn from_slices(slices: &[&[u8]]) -> Self {
        debug_assert!(
            slices.len() == 2,
            "System expected 2 inputs, found {}",
            slices.len()
        );

        let t_ptr = slices[0].as_ptr().cast::<T>();
        let u_ptr = slices[1].as_ptr().cast::<U>();
        // SAFETY: the caller guarantees `slices[0]` is sized, aligned and valid for `T`
        // and not mutably borrowed for `'a`. Cast and dereferenced on its own, with no
        // reference to `slices[1]`.
        let t = unsafe { &*t_ptr };
        // SAFETY: the same obligation discharged independently for `slices[1]` and `U`.
        // The two leaves are separate allocations belonging to separate producers, so
        // neither cast constrains the other.
        let u = unsafe { &*u_ptr };
        (t, u)
    }
}

// impl<'a, T, U> SystemDataOut<'a> for (&'a mut T, &'a mut U) {
//     unsafe fn from_slices_mut(slices: &mut [&'a mut [u8]]) -> Self {
//         debug_assert!(slices.len() >= 2);

//         let t_ptr = slices[0].as_mut_ptr() as *mut T;
//         let u_ptr = slices[1].as_mut_ptr() as *mut U;
//         unsafe { (&mut *t_ptr, &mut *u_ptr) }
//     }
// }
