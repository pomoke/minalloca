use std::{
    arch::naked_asm,
    mem::forget,
    ptr,
};

/// Allocates `[u8; count]` of memory on stack, then run
/// the given closure with the allocation.
///
/// # Safety
/// - Don't use a too large `count`. Even if the stack have enough space,
///   access may go over guard page,
///   and result in segmentation fault.
/// - Your program may inadvertently break, or have UBs.
///
/// # Known Caveats
/// - May not work with AddressSanitizer.
/// - Will not unwind properly on panic.
pub unsafe fn with_alloca_raw<F>(count: usize, callback: F)
where
    F: FnOnce(*mut u8),
{
    let call_fn = ptr::from_ref(&callback);
    forget(callback);

    unsafe {
        // SAFETY: `call_fn` is valid as we have called `forget`.
        alloca_trampoline(count, call_fn as *mut u8, callback_as_c::<F> as *mut u8);
    }

}

#[unsafe(naked)]
unsafe extern "C" fn alloca_trampoline(count: usize, callback: *mut u8, call_wrapper: *mut u8) {
    naked_asm!(
        "push rbp", // Epilogue - keep bp
        "push r12",
        "mov rbp, rsp",
        "mov r12, rsp", // Stash original sp on callee-save reg
        "and rsp, -16", // Align sp to 16-bytes
        "sub rsp, rdi", // reserve `count` of size
        "and rsp, -16", // Align sp to 16-bytes
        "mov rdi, rsp", // param 1 for wrapper
        "",             // callback function is both 2-nd arg
        "call rdx",
        "mov rsp, r12", //restore sp
        "pop r12",
        "pop rbp",      // restore frame
        "ret",
    )
}

unsafe extern "C" fn callback_as_c<F>(ptr: *mut u8, call: *mut u8)
where
    F: FnOnce(*mut u8),
{
    // SAFETY: I don't know if it works.
    let closure: F = unsafe { ptr::read(call as *mut F) };
    closure(ptr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::slice;

    #[test]
    fn test_alloca_run() {
        unsafe {
            with_alloca_raw(128, |ptr| {
                println!("hello, world!");
            });
        }
    }

    #[test]
    fn test_alloca_as_slice() {
        unsafe {
            with_alloca_raw(24, |ptr| {
                let bytes: &mut [u8] = slice::from_raw_parts_mut(ptr, 24);
                for i in bytes.iter_mut() {
                    *i = '/' as u8;
                }
                let s = str::from_utf8(bytes).unwrap();
                println!("{}", s);

                println!("hello, world!");
            });
        }
    }

    #[test]
    fn test_alloca_closure() {
        unsafe {
            let mut a = String::new();
            with_alloca_raw(24, |ptr| {
                let bytes: &mut [u8] = slice::from_raw_parts_mut(ptr, 24);
                for i in 0..4 {
                    a.push_str("ab");
                }
            });
            assert_eq!(a, "abababab");
        }
    }

    #[test]
    #[should_panic]
    fn test_alloca_unwind() {
        unsafe {
            with_alloca_raw(24, |ptr| panic!("test panic"));
        }
    }
}
