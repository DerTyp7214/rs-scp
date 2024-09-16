fn main() {
    if cfg!(target_os = "windows") {
        println!("cargo:rustc-link-search=libs/windows/x86_64");
        println!("cargo:rustc-link-lib=ssh");
    } else if cfg!(target_os = "linux") {
        println!("cargo:rustc-link-search=libs/linux/x86_64");
        println!("cargo:rustc-link-lib=ssh");
    } else if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-search=libs/macos/x86_64");
        println!("cargo:rustc-link-lib=ssh");
    }
}