//! Open up a project shell

use crate::builder;
use crate::builder::RunStatus;
use crate::cas::ContentAddressable;
use crate::cli::ShellOptions;
use crate::nix::CallOpts;
use crate::ops::error::{ExitError, OpResult};
use crate::project::{roots::Roots, Project};
use crossbeam_channel as chan;
use slog_scope::debug;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use std::{env, thread};

/// This is the entry point for the `lorri shell` command.
///
/// # Overview
///
/// `lorri shell` launches the user's shell with the project environment set up. "The user's shell"
/// here just means whatever binary $SHELL points to. Concretely we get the following process tree:
///
/// `lorri shell`
/// ├── builds the project environment if --cached is false
/// ├── writes a bash init script that loads the project environment
/// ├── SPAWNS bash with the init script as its `--rcfile`
/// │   └── EXECS `lorri start_user_shell_`
/// │       ├── (*) performs shell-specific setup for $SHELL
/// │       └── EXECS into user shell $SHELL
/// │           └── interactive user shell
/// └── `lorri shell` terminates
///
/// This setup allows lorri to support almost any shell with minimal additional work. Only the step
/// marked (*) must be adjusted, and only in case we want to customize the shell, e.g. changing the
/// way the prompt looks.
pub fn main(project: Project, opts: ShellOptions) -> OpResult {
    let lorri = env::current_exe().expect("failed to determine lorri executable's path");
    let shell = env::var("SHELL").expect("lorri shell requires $SHELL to be set");
    debug!("using shell path {}", shell);

    let mut bash_cmd = bash_cmd(
        if opts.cached {
            cached_root(&project)?
        } else {
            build_root(&project)?
        },
        &project.cas,
    )?;
    debug!("bash"; "command" => ?bash_cmd);
    bash_cmd
        .args(&[
            "-c",
            "exec \"$1\" start_user_shell_ --shell-path=\"$2\" --shell-file=\"$3\"",
            "--",
            &lorri
                .to_str()
                .expect("lorri executable path not UTF-8 clean"),
            &shell,
            &PathBuf::from(&project.nix_file)
                .to_str()
                .expect("Nix file path not UTF-8 clean"),
        ])
        .status()
        .expect("failed to execute bash");
    Ok(())
}

fn build_root(project: &Project) -> Result<PathBuf, ExitError> {
    let (tx, rx) = chan::unbounded();
    thread::spawn(move || {
        eprint!("lorri: building environment");
        let mut last = Instant::now();
        for msg in rx {
            // Set the maximum rate of the "progress bar"
            if last.elapsed() >= Duration::from_millis(500) {
                eprint!(".");
                io::stderr().flush().unwrap();
                last = Instant::now();
            }
            debug!("build"; "message" => ?msg);
        }
        eprintln!(". done");
    });

    let run_result = builder::run(tx, &project.nix_file, &project.cas)
        .map_err(|e| ExitError::temporary(format!("build failed: {:?}", e)))?;
    Ok(Path::new(
        match run_result.status {
            RunStatus::Complete(build) => Roots::from_project(&project)
                .create_roots(build)
                .map_err(|e| {
                    ExitError::temporary(format!("rooting the environment failed: {:?}", e))
                }),
            e => Err(ExitError::temporary(format!("build failed: {:?}", e))),
        }?
        .shell_gc_root
        .as_os_str(),
    )
    .to_owned())
}

fn cached_root(project: &Project) -> Result<PathBuf, ExitError> {
    let root_paths = Roots::from_project(&project).paths();
    if !root_paths.all_exist() {
        Err(ExitError::temporary(
            "project has not previously been built successfully",
        ))
    } else {
        Ok(Path::new(root_paths.shell_gc_root.as_os_str()).to_owned())
    }
}

/// Instantiates a `Command` to start bash.
pub fn bash_cmd(project_root: PathBuf, cas: &ContentAddressable) -> Result<Command, ExitError> {
    let init_file = cas
        .file_from_string(&format!(
            r#"
EVALUATION_ROOT="{}"

{}"#,
            project_root.display(),
            include_str!("direnv/envrc.bash")
        ))
        .expect("failed to write shell output");

    debug!("building bash via runtime closure"; "closure" => crate::RUN_TIME_CLOSURE);
    let bash_path = CallOpts::expression(&format!("(import {}).path", crate::RUN_TIME_CLOSURE))
        .value::<PathBuf>()
        .expect("failed to get runtime closure path");

    let mut cmd = Command::new(bash_path.join("bash"));
    cmd.env(
        "BASH_ENV",
        init_file
            .to_str()
            .expect("script file path not UTF-8 clean"),
    );
    Ok(cmd)
}
