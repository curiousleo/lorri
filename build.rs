//! build.rs is used to generate code at build time, which is then
//! imported elsewhere. This file is understood and executed by cargo.
use std::env;
use std::fs::File;
use std::io::Write;
use std::path::Path;

/// write out to a build_rev.rs file, which contains generated
/// code containing information about the built version.
/// build_rev.rs is included in src/lib.rs.
fn main() {
    println!("cargo:rerun-if-env-changed=BUILD_REV_COUNT");
    println!("cargo:rerun-if-changed=build.rs");
    // OUT_DIR is set by cargo:
    // https://doc.rust-lang.org/cargo/reference/environment-variables.html
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("build_rev.rs");
    let mut f = File::create(&dest_path).unwrap();

    let rev_count = env::var("BUILD_REV_COUNT")
        .expect("BUILD_REV_COUNT not set, please reload nix-shell")
        .parse::<usize>()
        .expect("BUILD_REV_COUNT should be parsable as usize");

    f.write_all(
        format!(
            r#"
/// Number of revisions in the Git tree.
pub const VERSION_BUILD_REV: usize = {};
"#,
            rev_count
        )
        .as_bytes(),
    )
    .unwrap();
}
