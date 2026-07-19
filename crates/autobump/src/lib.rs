pub mod bump;

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use std::{
        alloc::{Layout, alloc},
        array,
    };

    use crate::bump::Bump;

    #[test]
    fn test_raw_bump() {
        let mem = unsafe { alloc(Layout::new::<[u8; 1024]>()) };
        let bump = unsafe { Bump::unsafe_new(mem, mem.add(1024)) };
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
        let bump = unsafe { Bump::unsafe_new(mem, mem.add(1024)) };

        let scope_a = bump.scope();
        let a = scope_a.put(1f32);
        let scope_b = bump.scope();
        let b = scope_b.put(2f32);
        drop(a);
        drop(scope_a);
    }

    #[test]
    #[should_panic]
    fn test_bump_innermost_alloc_only() {
        let mem = unsafe { alloc(Layout::new::<[u8; 1024]>()) };
        let bump = unsafe { Bump::unsafe_new(mem, mem.add(1024)) };

        let scope_a = bump.scope();
        let _a = scope_a.put(1f32);
        let scope_b = bump.scope();
        let _b = scope_b.put(2f32);
        let _c = scope_a.put(2f32);
    }
}
