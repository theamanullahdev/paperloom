//! Embeds the Paperloom icon in the Windows executable, so the desktop and
//! Start menu shortcuts, the taskbar and Task Manager show it.

fn main() {
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../packaging/windows/paperloom.ico");
        resource.set("ProductName", "Paperloom");
        resource.set("FileDescription", "Paperloom");
        if let Err(err) = resource.compile() {
            println!("cargo:warning=could not embed the Windows icon: {err}");
        }
    }
}
