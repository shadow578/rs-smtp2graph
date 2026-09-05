#[cfg(windows)]
extern crate winres;

fn main() {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=icon_c.ico");
        println!("cargo:rerun-if-changed=manifest.xml");
        println!("cargo:rerun-if-changed=Cargo.toml");
        
        // only build resources for release
        if std::env::var("PROFILE").unwrap() == "release" {
            let mut res = winres::WindowsResource::new();
            res.set_icon("icon_c.ico")
                .set_manifest_file("manifest.xml")
                .set_language(0x0409); // MAKELANGID(LANG_ENGLISH, SUBLANG_ENGLISH_US )
            if let Err(e) = res.compile() {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
    }
}
