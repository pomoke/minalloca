use core::slice;

use dynstk::alloca_raw_with;

fn main() {
    unsafe {alloca_raw_with(32, |ptr| {
        let bytes: &mut [u8] = slice::from_raw_parts_mut(ptr, 32);
        for i in bytes.iter_mut() {
            *i = 'a' as u8;
        }
        let s = str::from_utf8(bytes).unwrap();
        println!("{}",s);

        println!("hello, world!");
    });}
}
