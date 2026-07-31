use std::{
    alloc::{Layout, alloc_zeroed, dealloc, handle_alloc_error},
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

/// The solver's per-system output buffers, plus the one borrow operation it needs.
///
/// # Why this type exists
///
/// Each step, a system needs its own output buffer borrowed **uniquely** while the
/// buffers of its producers are borrowed **shared**, all at once. Expressed directly
/// over a `Vec<AlignedBuffer>` that is a disjoint-index borrow, which the borrow
/// checker cannot see is disjoint.
///
/// The solver used to solve this by casting a shared buffer's `as_ptr()` to `*mut u8`
/// and calling `from_raw_parts_mut` on it. That was undefined behaviour — a shared
/// borrow grants no write permission, so the cast retagged a `SharedReadOnly` tag as
/// Unique — and it put `unsafe` in the solver, which `specs/mission.md` Article 1
/// forbids independently of whether it is sound.
///
/// The operation lives here instead, in a module Article 1 designates for buffer
/// allocation, so the aliasing argument sits next to the allocation it depends on.
///
/// # What makes it sound
///
/// Every `AlignedBuffer` owns a **separate heap allocation**. The `Vec` holds only the
/// `(ptr, layout, len)` structs; the bytes a caller reads and writes live elsewhere,
/// one allocation per system. So once the input references have been taken and the
/// borrows of the `Vec` itself released, borrowing one buffer mutably cannot invalidate
/// them: it touches a different allocation entirely.
///
/// That is why [`BufferSet::with_split`] needs exactly one `unsafe` operation — a
/// lifetime extension — rather than a raw-pointer reconstruction of the whole
/// borrowing pattern.
pub(crate) struct BufferSet {
    buffers: Vec<AlignedBuffer>,
    /// Scratch for the input slice handed to `with_split`'s callback, kept here so the
    /// allocation is reused across steps instead of being rebuilt per system per step.
    ///
    /// The `'static` is not true. It is a lifetime extension performed inside
    /// `with_split` and valid only for that call. The invariant that makes it
    /// tolerable: **this vector is empty except while `with_split` is running.** It is
    /// cleared on entry and on exit, never returned, never borrowed out, and no other
    /// method touches it.
    scratch: Vec<&'static [u8]>,
}

impl BufferSet {
    /// Allocate one buffer per system, from each system's reported size and alignment.
    pub(crate) fn new(sizes_and_aligns: impl Iterator<Item = (usize, usize)>) -> Self {
        Self {
            buffers: sizes_and_aligns
                .map(|(size, align)| AlignedBuffer::new(size, align))
                .collect(),
            scratch: Vec::new(),
        }
    }

    /// Borrow buffer `out` mutably and every buffer named in `inputs` immutably, for
    /// the duration of `f`.
    ///
    /// The callback shape is deliberate. An equivalent
    /// `-> (&mut [u8], Vec<&[u8]>)` would force a fresh allocation per call: the
    /// returned references carry a lifetime tied to a single `&mut self`, and a vector
    /// hoisted out of the solver's loop cannot hold references shorter-lived than
    /// itself. Keeping the borrows inside a callback lets the scratch storage live in
    /// `self` and be reused.
    ///
    /// # Panics
    ///
    /// Panics if `out` or any index in `inputs` is out of range, or if `out` appears in
    /// `inputs`.
    ///
    /// These are `assert!` rather than `debug_assert!` even though the invariant is one
    /// this crate maintains — `SystemPool::link` rejects a system that feeds itself, so
    /// user code cannot reach a violation directly. `specs/mission.md` Article 4 would
    /// permit `debug_assert!` on that reading. It is always-on regardless, because it
    /// is the precondition the lifetime extension below relies on: a soundness check
    /// compiled out in release converts a panic into undefined behaviour in exactly the
    /// build where it is least diagnosable.
    pub(crate) fn with_split<R>(
        &mut self,
        out: usize,
        inputs: &[usize],
        f: impl FnOnce(&mut [u8], &[&[u8]]) -> R,
    ) -> R {
        let count = self.buffers.len();
        assert!(
            out < count,
            "output buffer index {out} out of range (pool has {count} systems)"
        );

        self.scratch.clear();

        for &idx in inputs {
            assert!(
                idx < count,
                "input buffer index {idx} out of range (pool has {count} systems)"
            );
            assert!(
                idx != out,
                "system {out} would borrow its own output buffer as an input; \
                 SystemPool::link rejects self-feeding links, so reaching this means \
                 the link table was built incorrectly"
            );

            let slice: &[u8] = &self.buffers[idx];

            // SAFETY: extends `slice` to `'static` so it can be stored in `self.scratch`
            // and the allocation reused across calls. The reference does not actually
            // live that long; three things keep it valid for as long as it is readable:
            //
            // 1. It points into `self.buffers[idx]`'s own heap allocation, made by
            //    `AlignedBuffer::new` and freed only by its `Drop`. `self` is borrowed
            //    for this whole call, so no buffer can be added, removed or dropped
            //    while the extended reference exists.
            // 2. The only mutable borrow taken below is of `self.buffers[out]`, and
            //    `idx != out` is asserted above. Because each buffer owns a separate
            //    allocation, that borrow cannot alias these bytes even transiently.
            // 3. The scratch is cleared on entry and again on exit, and is never
            //    returned or borrowed out of this type, so no extended reference is
            //    observable after `f` returns.
            let extended: &'static [u8] = unsafe { std::mem::transmute::<&[u8], &[u8]>(slice) };

            self.scratch.push(extended);
        }

        // Field-level split borrow: `self.buffers` mutably, `self.scratch` immutably.
        // Both are safe references; the covariance of `&[&'static [u8]]` into
        // `&[&'a [u8]]` is what lets the scratch be handed to `f` without a second cast.
        let output: &mut [u8] = &mut self.buffers[out];
        let result = f(output, &self.scratch);

        self.scratch.clear();
        result
    }
}

// This struct owns the memory.
// We use u128 to guarantee 16-byte alignment, which covers
// u8, u16, u32, u64, f64, and most SIMD types.
pub struct AlignedBuffer {
    ptr: NonNull<u8>,
    layout: Layout,
    len: usize,
}

impl AlignedBuffer {
    /// Allocate a zeroed buffer of `size` bytes at `align` byte alignment.
    ///
    /// A `size` of zero does not allocate: the buffer holds a dangling but well-aligned
    /// pointer and a zero length, which `Deref` turns into a valid empty slice. This is
    /// the case for a block whose `Output` is `()`.
    ///
    /// The memory is zero-initialised, which is what gives every block a defined
    /// starting output before its first update. Note that this makes the all-zero bit
    /// pattern part of the safety argument for `SystemDataOut::from_slices_mut`: see
    /// that declaration's `# Safety` section.
    ///
    /// # Panics
    ///
    /// Panics if `Layout::from_size_align(size, align)` rejects the pair — that is, if
    /// `align` is not a power of two, or if rounding `size` up to `align` would exceed
    /// `isize::MAX`. Both are caller errors rather than conditions this crate can
    /// recover from: `align` reaches here from `RawSystem::output_alignment`, which is
    /// `align_of` an actual type and therefore always a valid power of two, so a panic
    /// means the `RawSystem` impl was hand-written and wrong.
    ///
    /// Allocation failure is *not* a panic — it routes to `handle_alloc_error`.
    pub fn new(size: usize, align: usize) -> Self {
        if size == 0 {
            // Handle zero-sized buffers gracefully without allocating.
            // `Layout::new::<()>()` is the size-0 align-1 layout, built in const and
            // without a fallible constructor to unwrap.
            return Self {
                ptr: NonNull::dangling(),
                layout: Layout::new::<()>(),
                len: 0,
            };
        }

        // Create the layout.
        // valid alignments are powers of 2.
        let layout = Layout::from_size_align(size, align)
            .expect("Invalid layout: size exceeds limits or align is not power of 2");

        // SAFETY: `alloc_zeroed` requires a layout of non-zero size. `size == 0`
        // returned above, and `Layout::from_size_align` succeeded, so `layout` is a
        // valid layout of exactly `size > 0` bytes.
        let raw_ptr = unsafe { alloc_zeroed(layout) };

        // Handle Out-Of-Memory (OOM)
        let ptr = NonNull::new(raw_ptr).unwrap_or_else(|| handle_alloc_error(layout));

        Self {
            ptr,
            layout,
            len: size,
        }
    }
}

impl Drop for AlignedBuffer {
    fn drop(&mut self) {
        if self.layout.size() != 0 {
            // SAFETY: `dealloc` requires a pointer from this allocator allocated with
            // the very layout passed here. `self.layout.size() != 0` excludes the
            // dangling zero-size case, so `self.ptr` can only have come from the
            // `alloc_zeroed(layout)` in `new`, and `self.layout` is the same `Layout`
            // value stored at that point — neither field is reachable or mutable from
            // outside this module. `Drop` runs once, so the pointer is not freed twice.
            unsafe {
                // Free the memory using the exact same layout used to create it
                dealloc(self.ptr.as_ptr(), self.layout);
            }
        }
    }
}

// Allow it to behave like a slice
impl Deref for AlignedBuffer {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        // SAFETY: `from_raw_parts` requires `self.len` bytes readable from
        // `self.ptr`, properly aligned, initialised, and not mutated for the returned
        // lifetime. `new` sets `len` to the allocation size and never changes it, and
        // `alloc_zeroed` initialised every one of those bytes. `u8` has alignment 1, so
        // any non-null pointer is aligned — including the `NonNull::dangling()` of the
        // zero-size case, where `len` is 0 and an empty slice is valid. The returned
        // slice borrows `&self`, so the compiler forbids a concurrent `&mut`.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }
}

impl DerefMut for AlignedBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: the same size, alignment and initialisation argument as `Deref`
        // above. Exclusivity, which `from_raw_parts_mut` additionally requires, comes
        // from `&mut self`: this buffer owns the allocation, no other `AlignedBuffer`
        // can point at it, and the returned slice's lifetime is tied to the `&mut`.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }
}
