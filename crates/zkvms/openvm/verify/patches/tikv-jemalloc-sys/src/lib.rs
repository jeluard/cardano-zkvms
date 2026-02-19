// WASM-compatible stub for tikv-jemalloc-sys.
// Delegates all allocation to Rust's standard allocator on wasm32 targets.

#![allow(non_camel_case_types)]

use std::alloc;

pub use std::ffi::{c_int, c_void};

#[cfg(target_arch = "wasm32")]
pub mod ffi {
    use std::alloc;
    use std::ffi::{c_int, c_void};
    use std::mem::Layout;

    pub const MALLOCX_LG_ALIGN_SHIFT: i32 = 0;
    pub const MALLOCX_ALIGN_SHIFT: i32 = 1;
    pub const MALLOCX_ZERO_SHIFT: i32 = 10;

    #[allow(non_snake_case)]
    pub fn MALLOCX_ALIGN(align: usize) -> i32 {
        if align == 0 {
            0
        } else {
            let mut v = align;
            let mut result = 0;
            while v > 1 {
                v >>= 1;
                result += 1;
            }
            (result << 1) | 0
        }
    }

    #[allow(non_snake_case)]
    pub fn MALLOCX_ZERO() -> i32 {
        1 << MALLOCX_ZERO_SHIFT
    }

    pub const ALIGNOF_MAX_ALIGN_T: usize = 16;
    pub const JEMALLOC_VERSION: &str = "5.3.0-stub-wasm";

    #[no_mangle]
    pub unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
        if size == 0 {
            return std::ptr::null_mut();
        }
        let layout = Layout::from_size_align_unchecked(size, 1);
        alloc::alloc(layout) as *mut c_void
    }

    #[no_mangle]
    pub unsafe extern "C" fn free(_ptr: *mut c_void) {
        // Accept memory leaks: C FFI doesn't provide size info for safe dealloc.
    }

    #[no_mangle]
    pub unsafe extern "C" fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
        if size == 0 {
            return std::ptr::null_mut();
        }
        let new_layout = Layout::from_size_align_unchecked(size, 1);
        if ptr.is_null() {
            alloc::alloc(new_layout) as *mut c_void
        } else {
            let old_layout = Layout::new::<u8>();
            alloc::realloc(ptr as *mut u8, old_layout, size) as *mut c_void
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn calloc(count: usize, size: usize) -> *mut c_void {
        let total_size = count.saturating_mul(size);
        if total_size == 0 {
            return std::ptr::null_mut();
        }
        let layout = Layout::from_size_align_unchecked(total_size, 1);
        alloc::alloc_zeroed(layout) as *mut c_void
    }

    #[no_mangle]
    pub unsafe extern "C" fn mallocx(size: usize, _flags: i32) -> *mut c_void {
        malloc(size)
    }

    #[no_mangle]
    pub unsafe extern "C" fn rallocx(ptr: *mut c_void, size: usize, _flags: i32) -> *mut c_void {
        realloc(ptr, size)
    }

    #[no_mangle]
    pub unsafe extern "C" fn xallocx(_ptr: *mut c_void, size: usize, _extra: usize, _flags: i32) -> usize {
        size
    }

    #[no_mangle]
    pub unsafe extern "C" fn sdallocx(ptr: *mut c_void, _size: usize, _flags: i32) {
        if !ptr.is_null() {
            let layout = Layout::new::<u8>();
            alloc::dealloc(ptr as *mut u8, layout);
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn malloc_usable_size(ptr: *const c_void) -> usize {
        if ptr.is_null() { 0 } else { 1 }
    }

    #[no_mangle]
    pub unsafe extern "C" fn nallocx(size: usize, _flags: i32) -> usize {
        size
    }

    #[no_mangle]
    pub unsafe extern "C" fn malloc_stats_print(
        _write: *const c_void,
        _cbopaque: *mut c_void,
        _opts: *const u8,
    ) {
    }

    #[no_mangle]
    pub unsafe extern "C" fn mallctl(
        _name: *const u8,
        _oldp: *mut c_void,
        _oldlenp: *mut usize,
        _newp: *mut c_void,
        _newlen: usize,
    ) -> c_int {
        -1
    }
}

#[cfg(not(target_arch = "wasm32"))]
compile_error!("This stub should only be used for WASM targets.");

#[cfg(target_arch = "wasm32")]
pub use ffi::*;

#[cfg(target_arch = "wasm32")]
pub mod __static {
    pub use super::ffi::{
        ALIGNOF_MAX_ALIGN_T, MALLOCX_ALIGN_SHIFT, MALLOCX_LG_ALIGN_SHIFT, MALLOCX_ZERO_SHIFT,
    };
}

#[cfg(target_arch = "wasm32")]
pub use __static::*;
