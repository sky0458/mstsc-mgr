#[cfg(windows)]
fn main() {
    println!("cargo:rerun-if-changed=assets/mstsc-mgr.ico");
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/mstsc-mgr.ico");
    resource.set("ProductName", "mstsc-mgr external");
    resource.set("FileDescription", "External MSTSC account manager");
    resource
        .compile()
        .unwrap_or_else(|error| panic!("failed to compile Windows resources: {error}"));
}

#[cfg(not(windows))]
fn main() {}
