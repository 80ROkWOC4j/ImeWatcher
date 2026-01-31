fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=build/windows-manifest.rc");
    println!("cargo:rerun-if-changed=icon.ico");

    let result = embed_resource::compile("build/windows-manifest.rc", embed_resource::NONE);
    result.manifest_required().unwrap_or_else(|e| {
        panic!(
            "Windows resource embedding failed ({e}). \
This breaks single-exe distribution (icon/manifest may be missing). \
Install MSVC + Windows SDK (rc.exe/llvm-rc available) and rebuild."
        )
    });
}
