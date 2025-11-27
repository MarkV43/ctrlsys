use std::{
    alloc::{Layout, alloc_zeroed, dealloc, handle_alloc_error},
    ops::{Deref, DerefMut},
    ptr::NonNull,
    slice,
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
    /// Allocate a buffer with specific size (bytes) and alignment (bytes).
    /// `align` must be a power of two.
    pub fn new(size: usize, align: usize) -> Self {
        if size == 0 {
            // Handle zero-sized buffers gracefully without allocating
            return Self {
                ptr: NonNull::dangling(),
                layout: Layout::from_size_align(0, 1).unwrap(),
                len: 0,
            };
        }

        // Create the layout.
        // valid alignments are powers of 2.
        let layout = Layout::from_size_align(size, align)
            .expect("Invalid layout: size exceeds limits or align is not power of 2");

        unsafe {
            // We use alloc_zeroed because your logic expects 0-initialized memory
            // (similar to vec![0; n])
            let raw_ptr = alloc_zeroed(layout);

            // Handle Out-Of-Memory (OOM)
            let ptr = NonNull::new(raw_ptr).unwrap_or_else(|| handle_alloc_error(layout));

            Self {
                ptr,
                layout,
                len: size,
            }
        }
    }
}

impl Drop for AlignedBuffer {
    fn drop(&mut self) {
        if self.layout.size() != 0 {
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
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }
}

impl DerefMut for AlignedBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }
}
