use std::{
    alloc::{Layout, alloc_zeroed, dealloc, handle_alloc_error},
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

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
