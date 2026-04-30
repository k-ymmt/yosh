//! Asserts the WIT file's leading `package yosh:plugin@<x.y.z>[-pre];`
//! declaration is well-formed. release.sh's phase_publish_wit relies on
//! this shape (sed rewrite + grep -v selector). A malformed WIT here
//! would silently break the publish pipeline; this test catches it
//! during ordinary `cargo test -p yosh-plugin-api`.

#[test]
fn wit_starts_with_package_declaration() {
    let wit_path = concat!(env!("CARGO_MANIFEST_DIR"), "/wit/yosh-plugin.wit");
    let wit = std::fs::read_to_string(wit_path).expect("read wit");

    let first_line = wit
        .lines()
        .find(|l| !l.trim().is_empty())
        .expect("WIT file must have a non-empty line");

    let prefix = "package yosh:plugin@";
    let suffix = ";";
    assert!(
        first_line.starts_with(prefix),
        "first non-blank line missing 'package yosh:plugin@' prefix: {first_line:?}"
    );
    assert!(
        first_line.ends_with(suffix),
        "first non-blank line missing trailing ';': {first_line:?}"
    );

    let ver = &first_line[prefix.len()..first_line.len() - suffix.len()];
    let core: &str = ver.split('-').next().expect("non-empty version");
    let parts: Vec<&str> = core.split('.').collect();
    assert_eq!(
        parts.len(),
        3,
        "version core must be x.y.z, got {ver:?}"
    );
    for p in &parts {
        assert!(
            !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()),
            "version component is not a non-empty numeric: {p:?}"
        );
    }
}
