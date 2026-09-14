fn main() {
    // ggml's Metal backend guards APIs with `@available`, which compiles to
    // `___isPlatformVersionAtLeast` from clang's compiler runtime. Rust links
    // with -nodefaultlibs, so that runtime has to be named or the link fails.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        if let Some(dir) = clang_resource_dir() {
            println!("cargo:rustc-link-search=native={dir}/lib/darwin");
            println!("cargo:rustc-link-lib=static=clang_rt.osx");
        }
    }
    tauri_build::build()
}

fn clang_resource_dir() -> Option<String> {
    let out = std::process::Command::new("clang")
        .arg("--print-resource-dir")
        .output()
        .ok()?;
    let dir = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!dir.is_empty()).then_some(dir)
}
