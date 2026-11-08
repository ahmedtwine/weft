use std::{env, fs, path::PathBuf};

#[test]
fn header_matches_source() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = dir.join("include").join("weft.h");
    let config = cbindgen::Config::from_file(dir.join("cbindgen.toml")).expect("cbindgen.toml");
    let mut generated = Vec::new();
    cbindgen::Builder::new()
        .with_crate(&dir)
        .with_config(config)
        .generate()
        .expect("header")
        .write(&mut generated);
    let generated = String::from_utf8(generated).expect("UTF-8");
    if env::var_os("WEFT_C_HEADER_WRITE").is_some() {
        fs::write(&path, &generated).expect("write header");
        return;
    }
    let existing = fs::read_to_string(&path).unwrap_or_default();
    assert!(
        existing == generated,
        "include/weft.h is out of date. Run `WEFT_C_HEADER_WRITE=1 cargo test -p weft-c`."
    );
}
