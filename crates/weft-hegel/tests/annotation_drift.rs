use std::{env, fs, path::PathBuf};

#[test]
fn the_annotation_files_of_the_sdk_match_the_source() {
    let sdk = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../sdk");
    for (file, generated) in [
        ("annotation.schema.json", weft_hegel::schema()),
        ("annotation.ts", weft_hegel::types()),
    ] {
        let path = sdk.join(file);
        if env::var_os("WEFT_ANNOTATION_WRITE").is_some() {
            fs::write(&path, &generated).expect("write annotation");
            continue;
        }
        assert!(
            fs::read_to_string(&path).unwrap_or_default() == generated,
            "sdk/{file} is out of date. Run `WEFT_ANNOTATION_WRITE=1 cargo nextest run -p weft-hegel`."
        );
    }
}
