fn main() {
    let mut attributes = tauri_build::Attributes::new();

    // tauri-build embeds the Common Controls v6 application manifest into the app
    // binary only. Test and example executables link the same tauri/wry/Win32 code
    // but without that manifest, so Windows loads Common Controls v5 and the
    // process dies at load time with STATUS_ENTRYPOINT_NOT_FOUND (0xC0000139).
    // Embed the manifest for every linked target instead, which fixes `cargo test`.
    // See https://github.com/tauri-apps/tauri/issues/13419.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        add_manifest();
    }

    tauri_build::try_build(attributes).expect("failed to run tauri-build");
}

fn add_manifest() {
    let manifest =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("windows-app-manifest.xml");

    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
}
