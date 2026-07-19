use std::{
    alloc::Layout,
    cell::UnsafeCell,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    ptr::drop_in_place,
    thread::panicking,
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

    pub fn scope<'a>(&'a self) -> BumpScope<'a> {
        BumpScope {
            bump: unsafe { self.unsafe_scope() },
            current: UnsafeCell::new(self.get_current()),
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
    /// Allocate a memory buffer.
    ///
    /// Returns `None` if aligning the current pointer or adding the allocation
    /// size would overflow, or if the allocation would exceed the bump limit.
    /// A failed allocation does not advance the bump pointer.
    ///
    /// It's up to caller to uphold scope rules.
    #[inline]
    pub fn checked_alloc(&self, layout: Layout) -> Option<*mut u8> {
        let align_mask = layout.align() - 1;
        let current = self.bump.get_current() as usize;
        let base = current.checked_add(align_mask)? & !align_mask;
        let next = base.checked_add(layout.size())?;

        if next > self.bump.limit as usize {
            return None;
        }

        self.bump.set_current(next as *mut u8);
        Some(base as *mut u8)
    }

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

    /// It's undefined to decllocate a inner scope when outer scope is in use.
    pub unsafe fn deallocate(&mut self) {
        self.bump.set_current(self.top);
    }
}

/// Dropping this scope restores bump pointer to the saved
/// value on creation,
/// without any form of runtime check.
impl<'a> Drop for UnsafeBumpScope<'a> {
    fn drop(&mut self) {
        unsafe {
            self.deallocate();
        }
    }
}

pub struct BumpScope<'a> {
    bump: UnsafeBumpScope<'a>,
    current: UnsafeCell<*mut u8>,
}

impl<'a> BumpScope<'a> {
    pub fn is_current(&self) -> bool {
        let ctx_current = unsafe { *self.current.get() };
        let global_current = self.bump.bump.get_current();
        ctx_current == global_current
    }

    /// Though checked, it's up to caller not to use return value
    /// when referring `BumpScope` goes out.
    pub unsafe fn alloc_raw(&self, layout: Layout) -> *mut u8 {
        if !self.is_current() {
            panic!("Must allocate from the innermost scope")
        }
        let ret = self.bump.checked_alloc(layout);
        unsafe { *self.current.get() = self.bump.bump.get_current() };

        ret.expect("no enough memory from bump")
    }

    pub fn alloc_ptr<'b>(&'b self, layout: Layout) -> BumpHandle<'b, u8, Self> {
        BumpHandle {
            ptr: unsafe { self.alloc_raw(layout) },
            _marker: PhantomData,
        }
    }

    pub fn put<'scope, T>(&'scope self, rhs: T) -> BumpHandle<'scope, T, Self> {
        let ptr = unsafe { self.alloc_raw(Layout::new::<T>()) } as *mut T;
        unsafe { *ptr = rhs };
        BumpHandle {
            ptr,
            _marker: PhantomData,
        }
    }

    /// Deallocate the scope, without checking for preconditions.
    ///
    /// It is Undefined to call this function violating scope rules.
    pub unsafe fn unsafe_release(&mut self) {
        unsafe {
            self.bump.deallocate();
        }
    }
}

impl<'a> Drop for BumpScope<'a> {
    fn drop(&mut self) {
        if !(self.is_current() || panicking()) {
            panic!("Drop order violation - release innermost scope first")
        }
    }
}

impl<'a> Scope for UnsafeBumpScope<'a> {}
impl<'a> Scope for BumpScope<'a> {}

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
