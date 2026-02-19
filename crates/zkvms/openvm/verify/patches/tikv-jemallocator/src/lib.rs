// WASM-compatible stub for tikv-jemallocator.
// Delegates all allocation to Rust's standard allocator on wasm32 targets.

#![allow(non_camel_case_types)]

#[cfg(target_arch = "wasm32")]
use std::alloc::{GlobalAlloc, Layout};

#[cfg(target_arch = "wasm32")]
pub struct Jemalloc;

#[cfg(target_arch = "wasm32")]
unsafe impl GlobalAlloc for Jemalloc {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        std::alloc::alloc(layout)
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        std::alloc::dealloc(ptr, layout)
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        std::alloc::alloc_zeroed(layout)
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, old_layout: Layout, new_size: usize) -> *mut u8 {
        std::alloc::realloc(ptr, old_layout, new_size)
    }
}
