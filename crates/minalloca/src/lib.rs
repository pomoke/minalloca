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
/// - Seems to unwind on panic, but it's unsure for now.
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
unsafe extern "C-unwind" fn alloca_trampoline(count: usize, callback: *mut u8, call_wrapper: *mut u8) {
    naked_asm!(
        ".cfi_startproc",
        "push rbp", // Epilogue - keep bp
        ".cfi_def_cfa rsp, 16",  // size of ret and rbp
        ".cfi_offset rbp, -16", // Stack grows down.
        "mov rbp, rsp",
        ".cfi_def_cfa_register rbp",
        "push r12",
        ".cfi_offset r12, -24",

        "mov r12, rsp", // Stash original sp on callee-save reg
        "and rsp, -16", // Align sp to 16-bytes
        "sub rsp, rdi", // reserve `count` of size
        "and rsp, -16", // Align sp to 16-bytes align
        "mov rdi, rsp", // param 1 for wrapper
        "",             // callback function is both 2-nd arg
        "call rdx",
        "mov rsp, r12", //restore sp
        "pop r12",
        "pop rbp",      // restore frame
        ".cfi_def_cfa_register rbp",
        "ret",
        ".cfi_endproc",
    )
}

unsafe extern "C-unwind" fn callback_as_c<F>(ptr: *mut u8, call: *mut u8)
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
            with_alloca_raw(128, |_ptr| {
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
            with_alloca_raw(24, |ptr| {
                let bytes: &mut [u8] = slice::from_raw_parts_mut(ptr, 24);
                for i in 0..8 {
                    bytes[i] = 'a' as u8;
                }
                let bytes = str::from_utf8_unchecked(&bytes[0..8]);
                assert_eq!(bytes, "aaaaaaaa");
            });
        }
    }

    #[test]
    #[should_panic]
    fn test_alloca_unwind() {
        unsafe {
            with_alloca_raw(24, |_ptr| panic!("test panic"));
        }
    }
}
