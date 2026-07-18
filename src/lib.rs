use std::{
    arch::{asm, naked_asm}, mem::{MaybeUninit, forget}, ptr,
};

/// Allocate memory on stack
/// 
/// # Safety
/// - Do not use a too large `count`. Even if the stack have enough space, 
///   access may go over guard page,
///   and result in segmentation fault.
/// - Your program may inadvertently break, or have UBs.
/// 
/// # Caveats 
/// - 
pub unsafe fn alloca_raw_with<F>(count: usize, callback: F)
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

    #[test]
    fn it_works() {
        unsafe {alloca_raw_with(128, |ptr| {
            println!("hello, world!");
        });}
    }
}
