#[cfg(windows)]
extern crate winres;

fn main() {
    #[cfg(windows)]
    {
        // only build resources for release
        if std::env::var("PROFILE").unwrap() == "release" {
            let mut res = winres::WindowsResource::new();
            res.set_icon("smtp2graph_a.ico")
                .set_manifest_file("manifest.xml")
                .set_language(0x0409); // MAKELANGID(LANG_ENGLISH, SUBLANG_ENGLISH_US )
            if let Err(e) = res.compile() {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
    }
}
