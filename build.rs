#[cfg(windows)]
fn main() -> std::io::Result<()> {
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("assets/mstsc-mgr.ico")
        .set("InternalName", "mstsc-mgr.exe")
        .set("OriginalFilename", "mstsc-mgr.exe")
        .set("ProductName", "mstsc-mgr");
    resource.compile()
}

#[cfg(not(windows))]
fn main() {}
