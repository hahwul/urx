use std::env;

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    // The unoptimized clap command builder exceeds the default Windows
    // executable stack. Reserve room for it before any CLI parsing runs.
    const STACK_BYTES: usize = 4 * 1024 * 1024;
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bin=urx=/STACK:{STACK_BYTES}");
    } else {
        println!("cargo:rustc-link-arg-bin=urx=-Wl,--stack,{STACK_BYTES}");
    }
}
