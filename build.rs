fn main() {
    embed_resource::compile("assets/made.exe.manifest.rc", embed_resource::NONE)
        .manifest_optional()
        .unwrap();
}
