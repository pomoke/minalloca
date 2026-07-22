# Minalloca

> This is **EXPERIMENTAL** software. Expect bugs, UBs and SEGVs.

`no_std` capable `alloca` for Rust, without need for C toolchain or `build.rs`.

Only works with **x86-64** and **sysv64** ABI currently.

## Usage
```rust
with_alloca_raw(128, |ptr| {
    // Your code here
});

```
