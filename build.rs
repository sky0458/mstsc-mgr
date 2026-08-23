#[cfg(windows)]
fn main() {
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("assets/mstsc-mgr.ico")
        .set("InternalName", "mstsc-mgr.exe")
        .set("OriginalFilename", "mstsc-mgr.exe")
        .set("ProductName", "mstsc-mgr");
    resource
        .compile()
        .expect("failed to compile Windows application resources");
}

#[cfg(not(windows))]
fn main() {}
