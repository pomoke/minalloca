use std::{arch::asm, mem::forget, ptr};

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
/// - Migration to `#[unsafe(naked)]` is ongoing.
pub unsafe fn with_alloca_raw<F>(count: usize, callback: F)
where
    F: FnOnce(*mut u8),
{
    let call_fn = ptr::from_ref(&callback);

    unsafe {
        asm!(
            "mov r12, rsp",
            "and rsp, -16",
            "mov rsi, rsp",
            "sub rsp, {0}",
            "mov rdi, rsp",
            "push rbp",
            "mov rbp, rsp",
            "mov rsi, {1}",
            "call {2}",
            "pop rbp",
            "mov rsp, r12",
            in(reg) count,
            in(reg) call_fn,
            in(reg) callback_as_c::<F>,
            out("r12") _,
            clobber_abi("sysv64")
        )
    };

    forget(callback);
}

extern "C" fn callback_as_c<F>(ptr: *mut u8, call: *mut u8)
where
    F: FnOnce(*mut u8),
{
    // SAFETY: I don't know if it works.
    let closure: F = unsafe { ptr::read(call as *mut F) };
    closure(ptr)
}

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
                    *i = 'a' as u8;
                }
                let s = str::from_utf8(bytes).unwrap();
                println!("{}", s);

                println!("hello, world!");
            });
        }
    }
}
