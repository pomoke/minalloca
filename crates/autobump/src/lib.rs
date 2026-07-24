#![cfg_attr(feature = "no_std", no_std)]
pub mod bump;

#[cfg(test)]
mod tests {
    use std::{
        alloc::{Layout, alloc},
        array,
    };

    use crate::bump::{Bump, BumpAllocError};

    #[test]
    fn test_raw_bump() {
        let mem = unsafe { alloc(Layout::new::<[u8; 1024]>()) };
        let bump = unsafe { Bump::unsafe_new(mem, 1024) };
        let scope_a = unsafe { bump.unsafe_scope() };
        let ranges: [_; 16] =
            array::from_fn(|_| unsafe { scope_a.unsafe_alloc(Layout::new::<f64>()) });
        println!("bump {:?}, a {:?}", bump, ranges);
        ranges.iter().zip(ranges.iter().skip(1)).for_each(|(a, b)| {
            let dist = unsafe { b.offset_from(*a) };
            assert!(dist >= 8);
        });
    }

    #[test]
    #[should_panic]
    fn test_bump_release() {
        let mem = unsafe { alloc(Layout::new::<[u8; 1024]>()) };
        let bump = unsafe { Bump::unsafe_new(mem, 1024) };

        let scope_a = bump.scope();
        let a = scope_a.put(1f32);
        let scope_b = bump.scope();
        let _b = scope_b.put(2f32);
        drop(a);
        drop(scope_a);
    }

    #[test]
    #[should_panic]
    fn test_bump_innermost_alloc_only() {
        let mem = unsafe { alloc(Layout::new::<[u8; 1024]>()) };
        let bump = unsafe { Bump::unsafe_new(mem, 1024) };

        let scope_a = bump.scope();
        let _a = scope_a.put(1f32);
        let scope_b = bump.scope();
        let _b = scope_b.put(2f32);
        let _c = scope_a.put(2f32);
    }

    #[test]
    fn try_alloc_reports_exhaustion_without_advancing() {
        let mut memory = [0_u8; 8];
        let start = memory.as_mut_ptr();
        let bump = unsafe { Bump::unsafe_new(start, memory.len()) };
        let scope = bump.scope();

        let first = unsafe { scope.try_alloc_raw(Layout::from_size_align(7, 1).unwrap()) };
        assert_eq!(first, Ok(start));
        assert_eq!(
            unsafe { scope.try_alloc_raw(Layout::new::<[u8; 2]>()) },
            Err(BumpAllocError::OutOfMemory),
        );

        // The failed allocation did not consume the final byte.
        assert_eq!(
            unsafe { scope.try_alloc_raw(Layout::new::<u8>()) },
            Ok(unsafe { start.add(7) }),
        );
    }

    #[test]
    fn try_alloc_reports_scope_order_violation() {
        let mut memory = [0_u8; 8];
        let start = memory.as_mut_ptr();
        let bump = unsafe { Bump::unsafe_new(start, memory.len()) };
        let outer = bump.scope();
        let inner = bump.scope();
        let inner_value = inner.try_put(42_u8).unwrap();

        assert_eq!(
            unsafe { outer.try_alloc_raw(Layout::new::<u8>()) },
            Err(BumpAllocError::ScopeOrderViolation),
        );
        assert_eq!(*inner_value, 42);
    }
}
