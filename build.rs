//! build.rs is used to generate code at build time, which is then
//! imported elsewhere. This file is understood and executed by cargo.
use std::env;
use std::fs;
use std::path::Path;

/// write out to a build_rev.rs file, which contains generated
/// code containing information about the built version.
/// build_rev.rs is included in src/lib.rs.
fn main() {
    println!("cargo:rerun-if-env-changed=BUILD_REV_COUNT");
    println!("cargo:rerun-if-env-changed=RUN_TIME_CLOSURE");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=VERSION");
    println!("cargo:rerun-if-changed=RUNTIME_CLOSURE");

    // OUT_DIR and CARGO_MANIFEST_DIR are set by cargo:
    // https://doc.rust-lang.org/cargo/reference/environment-variables.html
    let out_dir = Path::new(&env::var("OUT_DIR").unwrap()).to_path_buf();
    let manifest_dir = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).to_path_buf();

    // Read rev_count: try VERSION file, then BUILD_REV_COUNT environment variable.
    let rev_count = fs::read(manifest_dir.join("VERSION"))
        .map(|b| {
            String::from_utf8(b)
                .expect("VERSION file is not valid UTF-8")
                .trim()
                .parse::<usize>()
                .expect("VERSION file contents should be parseable as usize")
        })
        .unwrap_or_else(|_| {
            env::var("BUILD_REV_COUNT")
                .expect("BUILD_REV_COUNT not set, please reload nix-shell")
                .parse::<usize>()
                .expect("BUILD_REV_COUNT should be parseable as usize")
        });

    // Read runtime_closure: try RUNTIME_CLOSURE file, then RUN_TIME_CLOSURE environment variable.
    let runtime_closure = if manifest_dir.join("RUNTIME_CLOSURE").is_file() {
        manifest_dir
            .join("RUNTIME_CLOSURE")
            .to_str()
            .expect("RUNTIME_CLOSURE path is not UTF-8 clean")
            .to_string()
    } else {
        env::var("RUN_TIME_CLOSURE").expect("RUN_TIME_CLOSURE not set, please reload nix-shell")
    };

    fs::write(out_dir.join("build_rev.rs"),
        format!(
            r#"
/// Number of revisions in the Git tree.
pub const VERSION_BUILD_REV: usize = {};

/// Run-time closure parameters. This argument points to a file
/// generated by ./nix/runtime.nix in Lorri's source.
pub const RUN_TIME_CLOSURE: &str = "{}";
"#,
            rev_count, runtime_closure
        )
        .as_bytes(),
    )
    .unwrap();

    // Generate src/com_target_lorri.rs
    varlink_generator::cargo_build_tosource(
        "src/com.target.lorri.varlink",
        /* rustfmt */ true,
    );
}
