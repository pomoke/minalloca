use core::{
    alloc::Layout, cell::UnsafeCell, error::Error, fmt, marker::PhantomData, mem::MaybeUninit, ops::{Deref, DerefMut, Index}, ptr::{self, NonNull, drop_in_place}, slice::{from_raw_parts, from_raw_parts_mut},
};
use std::thread::panicking;

/// An error returned by the fallible bump allocation methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BumpAllocError {
    /// Aligning the current address or adding the allocation size overflowed.
    AddressOverflow,
    /// The allocation would extend past the bump's backing memory.
    OutOfMemory,
    /// An allocation was attempted through a scope that is not innermost,
    /// or attempt to release non-innermost scope.
    ScopeOrderViolation,
}

impl fmt::Display for BumpAllocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::AddressOverflow => "allocation address overflow",
            Self::OutOfMemory => "not enough memory in bump allocator",
            Self::ScopeOrderViolation => "allocation requires the innermost scope",
        })
    }
}

impl Error for BumpAllocError {}

fn try_allocation_range(
    current: usize,
    limit: usize,
    layout: Layout,
) -> Result<(usize, usize), BumpAllocError> {
    let align_mask = layout.align() - 1;
    let base = current
        .checked_add(align_mask)
        .ok_or(BumpAllocError::AddressOverflow)?
        & !align_mask;
    let next = base
        .checked_add(layout.size())
        .ok_or(BumpAllocError::AddressOverflow)?;

    if next > limit {
        return Err(BumpAllocError::OutOfMemory);
    }

    Ok((base, next))
}

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
    pub unsafe fn unsafe_new(from: *mut u8, len: usize) -> Self {
        Self {
            current: UnsafeCell::new(from),
            limit: unsafe { from.add(len) },
        }
    }

    /// Create a scope without enabling allocation-order checks.
    ///
    /// # Safety
    ///
    /// Scopes must be released in a subsequence of LIFO order, as documented
    /// on [`UnsafeBumpScope`].
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
    /// Try to allocate a memory buffer.
    ///
    /// This checks address arithmetic and the bump's backing-memory boundary,
    /// but deliberately does not check scope allocation order.
    /// A failed allocation does not advance the bump pointer.
    ///
    /// It's up to caller to uphold scope rules.
    #[inline]
    pub fn try_alloc(&self, layout: Layout) -> Result<*mut u8, BumpAllocError> {
        let (base, next) = try_allocation_range(
            self.bump.get_current() as usize,
            self.bump.limit as usize,
            layout,
        )?;

        self.bump.set_current(next as *mut u8);
        Ok(base as *mut u8)
    }

    /// Allocate a memory buffer, returning `None` on overflow or exhaustion.
    #[inline]
    pub fn checked_alloc(&self, layout: Layout) -> Option<*mut u8> {
        self.try_alloc(layout).ok()
    }

    /// Allocate a memory buffer without checking address arithmetic or bounds.
    ///
    /// # Safety
    ///
    /// The caller must ensure the aligned allocation fits in the bump's
    /// backing memory and must uphold the scope-order rules.
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

    /// Deallocate this scope without checking its position in the scope stack.
    ///
    /// # Safety
    ///
    /// This scope must be eligible for release according to the scope-order
    /// rules, and no released allocation may be used afterward.
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

    /// Try to allocate raw memory from this scope.
    ///
    /// # Safety
    ///
    /// The returned pointer must not be used after this `BumpScope` goes out
    /// of scope.
    pub unsafe fn try_alloc_raw(&self, layout: Layout) -> Result<*mut u8, BumpAllocError> {
        if !self.is_current() {
            return Err(BumpAllocError::ScopeOrderViolation);
        }

        let ptr = self.bump.try_alloc(layout)?;
        unsafe { *self.current.get() = self.bump.bump.get_current() };
        Ok(ptr)
    }

    /// Allocate raw memory, panic if the allocation fails.
    ///
    /// # Safety
    ///
    /// The returned pointer must not be used after this `BumpScope` goes out
    /// of scope.
    pub unsafe fn alloc_raw(&self, layout: Layout) -> *mut u8 {
        unsafe { self.try_alloc_raw(layout) }
            .unwrap_or_else(|error| panic!("failed to allocate from bump: {error}"))
    }

    pub fn try_alloc_ptr<'b>(
        &'b self,
        layout: Layout,
    ) -> Result<BumpHandle<'b, u8, Self>, BumpAllocError> {
        Ok(BumpHandle {
            ptr: unsafe { self.try_alloc_raw(layout)? },
            _marker: PhantomData,
        })
    }

    /// Allocate a handle of.
    pub fn alloc_ptr<'b>(&'b self, layout: Layout) -> BumpHandle<'b, u8, Self> {
        self.try_alloc_ptr(layout)
            .unwrap_or_else(|error| panic!("failed to allocate from bump: {error}"))
    }

    pub fn try_put<'scope, T>(
        &'scope self,
        rhs: T,
    ) -> Result<BumpHandle<'scope, T, Self>, BumpAllocError> {
        let ptr = unsafe { self.try_alloc_raw(Layout::new::<T>())? } as *mut T;
        unsafe { ptr.write(rhs) };
        Ok(BumpHandle {
            ptr,
            _marker: PhantomData,
        })
    }

    pub fn put<'scope, T>(&'scope self, rhs: T) -> BumpHandle<'scope, T, Self> {
        self.try_put(rhs)
            .unwrap_or_else(|error| panic!("failed to allocate from bump: {error}"))
    }

    /// Deallocate the scope, without checking for preconditions.
    ///
    /// # Safety
    ///
    /// This scope must be eligible for release according to the scope-order
    /// rules, and no released allocation may be used afterward.
    pub unsafe fn unsafe_release(&mut self) {
        unsafe {
            self.bump.deallocate();
        }
    }
}

impl<'a> Drop for BumpScope<'a> {
    fn drop(&mut self) {
        #[cfg(not(feature = "no_std"))]
        let normal = self.is_current() || panicking();
        #[cfg(feature = "no_std")]
        let normal = self.is_current();
        if !normal {
            panic!("Drop order violation - release innermost scope first")
        }
    }
}

impl<'a> Scope for UnsafeBumpScope<'a> {}
impl<'a> Scope for BumpScope<'a> {}

/// Handle of allocation for **`Sized`** type, bound to a `Scope`.
///
/// This handle prevents underlying scope to drop early.
/// Data is accessed by `Deref` and `DerefMut`.
///
/// Drops underlying data when going out of scope. To avoid automatically dropping,
/// use `ManuallyDrop<T>` for `T`.
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

pub struct BumpSliceRawHandle<'a, T, U: Scope> {
    ptr: NonNull<MaybeUninit<T>>,
    len: usize,
    _marker: PhantomData<&'a U>,
}

impl<'a, T, U: Scope> Deref for BumpSliceRawHandle<'a, T, U> {
    type Target = [MaybeUninit<T>];
    fn deref(&self) -> &Self::Target {
        unsafe { from_raw_parts(self.ptr.as_ptr(), self.len) }
    }
}

impl<'a, T, U: Scope> DerefMut for BumpSliceRawHandle<'a, T, U> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }
}

/// `Vec<T>` with initialized items accessible directly over Bump.
/// Only a subset of methods are provided compared to `std::vec::Vec`.
pub struct BumpSliceHandle<'a, T, U: Scope> {
    ptr: NonNull<T>,
    len: usize,
    inited_len: usize,
    _marker: PhantomData<&'a U>,
}

impl<'a, T, U: Scope> Deref for BumpSliceHandle<'a, T, U> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        unsafe { from_raw_parts(self.ptr.as_ptr(), self.inited_len) }
    }
}

impl<'a, T, U: Scope> DerefMut for BumpSliceHandle<'a, T, U> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { from_raw_parts_mut(self.ptr.as_ptr(), self.inited_len) }
    }
}

impl<'a, T, U: Scope> Drop for BumpSliceHandle<'a, T, U> {
    fn drop(&mut self) {
        unsafe {
            drop_in_place(ptr::slice_from_raw_parts_mut(self.ptr.as_ptr(), self.inited_len));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_range_reports_alignment_overflow() {
        let layout = Layout::from_size_align(0, 8).unwrap();
        assert_eq!(
            try_allocation_range(usize::MAX - 3, usize::MAX, layout),
            Err(BumpAllocError::AddressOverflow),
        );
    }

    #[test]
    fn allocation_range_reports_size_overflow() {
        let layout = Layout::from_size_align(2, 1).unwrap();
        assert_eq!(
            try_allocation_range(usize::MAX - 1, usize::MAX, layout),
            Err(BumpAllocError::AddressOverflow),
        );
    }
}
