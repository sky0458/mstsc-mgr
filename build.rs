#[cfg(windows)]
fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=assets/mstsc-mgr.ico");

    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("assets/mstsc-mgr.ico")
        .set("InternalName", "mstsc-mgr-external.exe")
        .set("OriginalFilename", "mstsc-mgr-external.exe")
        .set("ProductName", "mstsc-mgr external");
    resource.compile()
}

#[cfg(not(windows))]
fn main() {}
