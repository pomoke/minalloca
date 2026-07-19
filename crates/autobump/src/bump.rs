use std::{
    alloc::Layout,
    cell::UnsafeCell,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    ptr::drop_in_place,
};

#[derive(Debug)]
pub struct Bump {
    /// SAFETY: access to this cell is wrapped.
    current: UnsafeCell<*mut u8>,
    limit: *mut u8,
}

impl Bump {
    /// Make a bump from raw pointers.
    ///
    /// # Safety
    ///
    /// - The memory range specified by pointers must be valid.
    /// - `from <= to`
    /// - During `Bump` valid, the backlying range cannot be aliased or released.
    pub unsafe fn unsafe_new(from: *mut u8, to: *mut u8) -> Self {
        Self {
            current: UnsafeCell::new(from),
            limit: to,
        }
    }

    pub unsafe fn unsafe_scope<'a>(&'a self) -> UnsafeBumpScope<'a> {
        UnsafeBumpScope {
            top: unsafe { *self.current.get() },
            bump: self,
        }
    }

    fn set_current(&self, current: *mut u8) {
        unsafe { *self.current.get() = current }
    }

    fn get_current(&self) -> *mut u8 {
        unsafe { *self.current.get() }
    }
}

/// Marker trait for scopes
pub trait Scope {}

/// Unchecked Scope of Bump Allocator
///
/// # Safety
/// - Valid `Drop` order is any subsequence of LIFO order.
///     - The range dropped, and later ranges must not be used by any means, including `Drop`.
///     -  it is undefined to acquire a `BumpScope`, then drop a earlier created one.
/// - No runtime checking.
/// - It is possible to use any valid scope to get an allocation. However,
///   the allocation will be at the end of the innermost scope.
/// - To avoid misuse, `Bump::unsafe_scope()` is unsafe to make constraints explicit.
#[derive(Debug)]
pub struct UnsafeBumpScope<'a> {
    top: *mut u8,
    bump: &'a Bump,
}

impl<'a> UnsafeBumpScope<'a> {
    /// Alloc a memory buffer without checking preconditions.
    ///
    /// The fuction does not provide check for integer overflows
    /// or , and is unsafe.
    #[inline]
    pub unsafe fn unsafe_alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        // always power of 2 by Layout constraint
        let align = layout.align();

        let current = self.bump.get_current() as usize;
        let base = current.checked_add(align - 1).unwrap() & (!(align - 1));

        self.bump.set_current((base + size) as *mut u8);

        base as *mut u8
    }
}

/// Dropping this scope restores bump pointer to the saved
/// value on creation,
/// without any form of runtime check.
impl<'a> Drop for UnsafeBumpScope<'a> {
    fn drop(&mut self) {
        self.bump.set_current(self.top);
    }
}

pub struct BumpScope<'a> {
    bump: UnsafeBumpScope<'a>,
    current: *mut u8,
}

impl<'a> Scope for UnsafeBumpScope<'a> {}

pub struct BumpHandle<'a, T, U: Scope> {
    ptr: *mut T,
    _marker: PhantomData<&'a U>,
}

impl<'a, T, U: Scope> Deref for BumpHandle<'a, T, U> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr }
    }
}

impl<'a, T, U: Scope> DerefMut for BumpHandle<'a, T, U> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.ptr }
    }
}

impl<'a, T, U: Scope> Drop for BumpHandle<'a, T, U> {
    fn drop(&mut self) {
        unsafe {
            drop_in_place(self.ptr);
        }
    }
}
