use crossbeam_channel as chan;
use lorri::{
    builder::{self, RunStatus},
    cas::ContentAddressable,
    ops::shell,
    project::{roots::Roots, Project},
    NixFile,
};
use std::fs;
use std::iter::FromIterator;
use std::path::{Path, PathBuf};

#[test]
fn loads_env() {
    let tempdir = tempfile::tempdir().expect("tempfile::tempdir() failed us!");
    let project = project("loads_env", tempdir.path());
    let output = shell::bash_cmd(build(&project), &project.cas)
        .unwrap()
        .args(&["-c", "echo $MY_ENV_VAR"])
        .output()
        .expect("failed to run shell");

    assert_eq!(
        // The string conversion means we get a nice assertion failure message in case stdout does
        // not match what we expected.
        String::from_utf8(output.stdout).expect("stdout not UTF-8 clean"),
        "my_env_value\n"
    );
}

fn project(name: &str, cache_dir: &Path) -> Project {
    let test_root = PathBuf::from_iter(&[env!("CARGO_MANIFEST_DIR"), "tests", "shell", name]);
    let cas_dir = cache_dir.join("cas").to_owned();
    fs::create_dir_all(&cas_dir).expect("failed to create CAS directory");
    Project::new(
        NixFile::Shell(test_root.join("shell.nix")),
        &cache_dir.join("gc_roots").to_owned(),
        ContentAddressable::new(cas_dir).unwrap(),
    )
    .unwrap()
}

fn build(project: &Project) -> PathBuf {
    let (tx, _rx) = chan::unbounded();
    Path::new(
        match builder::run(tx, &project.nix_file, &project.cas)
            .unwrap()
            .status
        {
            RunStatus::Complete(build) => {
                Roots::from_project(&project).create_roots(build).unwrap()
            }
            _ => panic!("build failed"),
        }
        .shell_gc_root
        .as_os_str(),
    )
    .to_owned()
}
