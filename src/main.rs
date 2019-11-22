extern crate lorri;
extern crate structopt;
#[macro_use]
extern crate log;
#[macro_use]
extern crate human_panic;

use lorri::constants;
use lorri::locate_file;
use lorri::NixFile;

use lorri::cli::{Arguments, Command};
use lorri::ops::error::{ExitError, OpResult};
use lorri::ops::{daemon, direnv, info, init, ping, upgrade, watch};
use lorri::project::Project;
use std::path::PathBuf;
use structopt::StructOpt;

use cpp::cpp;

const TRIVIAL_SHELL_SRC: &str = include_str!("./trivial-shell.nix");
const DEFAULT_ENVRC: &str = "eval \"$(lorri direnv)\"";

cpp!{{
    #include "globals.hh"
    #include "shared.hh"
    #include "eval.hh"
    #include "eval-inline.hh"
    #include "get-drvs.hh"
    #include "attr-path.hh"
    #include "value-to-xml.hh"
    #include "value-to-json.hh"
    #include "util.hh"
    #include "store-api.hh"
    #include "common-eval-args.hh"
    #include "legacy.hh"

    #include <map>
    #include <iostream>
}}

fn _main(argc: i32, argv: *mut *mut u8) {
    let res = unsafe {
        cpp!([argc as "int", argv as "char * *"] -> i32 as "int" {

            using namespace nix;

            Strings files;

            struct MyArgs : LegacyArgs, MixEvalArgs
            {
                using LegacyArgs::LegacyArgs;
            };

            auto gcRoot;
            bool indirectRoot;

            MyArgs myArgs(baseNameOf(argv[0]), [&](Strings::iterator & arg, const Strings::iterator & end) {
                if (*arg == "--add-root")
                    gcRoot = getArg(*arg, arg, end);
                else if (*arg == "--indirect")
                    indirectRoot = true;
                else if (*arg != "" && arg->at(0) == '-')
                    return false;
                else
                    files.push_back(*arg);
                return true;
            });

            myArgs.parseCmdline(argvToStrings(argc, argv));

            // initPlugins();

            auto store = openStore();

            auto state = std::make_unique<EvalState>(myArgs.searchPath, store);
            state->repair = NoRepair;

            Bindings & autoArgs = *myArgs.getAutoArgs(*state);

            for (auto & i : files) {
                Expr * e = 
                    state->parseExprFromFile(resolveExprPath(state->checkSourcePath(lookupFileArg(*state, i))));
    //            processExpr(*state, /* attrPaths */ {""}, /* parseOnly */ false, /* strict */ false, autoArgs,
    //                /* evalOnly */ false, /* outputKind */ okPlain, /* xmlOutputSourceLocation */ true, e);
    //            processExpr(*state, autoArgs, e);



    Path rootName = absPath(gcRoot);
    auto store2 = state.store.dynamic_pointer_cast<LocalFSStore>();

    Value vRoot;
    state.eval(e, vRoot);
    state.forceValue(vRoot);

    DrvInfos drvs;
    getDerivations(state, vRoot, "", autoArgs, drvs, false);
    for (auto & i : drvs) {
        Path drvPath = i.queryDrvPath();

        /* What output do we want? */
        string outputName = i.queryOutputName();
        if (outputName == "")
            throw Error(format("derivation '%1%' lacks an 'outputName' attribute ") % drvPath);

        drvPath = store2->addPermRoot(drvPath, rootName, /* indirectRoot */ true);
        std::cout << format("%1%%2%\n") % drvPath % (outputName != "out" ? "!" + outputName : "");
    }



            }

            state->printStats();

            return 0;
        })
    };
}

fn main() {
    let mut hi = String::from("hi");
    _main(1, [hi.as_mut_ptr()].as_mut_ptr());
}

fn main_old() {
    // This returns 101 on panics, see also `ExitError::panic`.
    setup_panic!();

    let exit = |result: OpResult| match result {
        Err(err) => {
            eprintln!("{}", err.message());
            std::process::exit(err.exitcode());
        }
        Ok(Some(msg)) => {
            println!("{}", msg);
            std::process::exit(0);
        }
        Ok(None) => {
            std::process::exit(0);
        }
    };

    let opts = Arguments::from_args();

    lorri::logging::init_with_default_log_level(opts.verbosity);
    debug!("Input options: {:?}", opts);

    let result = run_command(opts);
    exit(result);
}

/// Try to read `shell.nix` from the current working dir.
fn get_shell_nix(shellfile: &PathBuf) -> Result<NixFile, ExitError> {
    // use shell.nix from cwd
    Ok(NixFile::from(locate_file::in_cwd(&shellfile).map_err(
        |_| {
            ExitError::user_error(format!(
                "`{}` does not exist\n\
                 You can use the following minimal `shell.nix` to get started:\n\n\
                 {}",
                shellfile.display(),
                TRIVIAL_SHELL_SRC
            ))
        },
    )?))
}

fn create_project(paths: &constants::Paths, shell_nix: NixFile) -> Result<Project, ExitError> {
    Project::new(shell_nix, &paths.gc_root_dir(), paths.cas_store().clone()).or_else(|e| {
        Err(ExitError::temporary(format!(
            "Could not set up project paths: {:#?}",
            e
        )))
    })
}

/// Run the main function of the relevant command.
fn run_command(opts: Arguments) -> OpResult {
    let paths = lorri::ops::get_paths()?;
    match opts.command {
        Command::Info(opts) => {
            get_shell_nix(&opts.nix_file).and_then(|sn| info::main(create_project(&paths, sn)?))
        }

        Command::Direnv(opts) => {
            get_shell_nix(&opts.nix_file).and_then(|sn| direnv::main(create_project(&paths, sn)?))
        }

        Command::Watch(opts) => get_shell_nix(&opts.nix_file)
            .and_then(|sn| watch::main(create_project(&paths, sn)?, opts)),

        Command::Daemon => daemon::main(),

        Command::Upgrade(opts) => upgrade::main(opts, paths.cas_store()),

        // TODO: remove
        Command::Ping_(opts) => get_shell_nix(&opts.nix_file).and_then(ping::main),

        Command::Init => init::main(TRIVIAL_SHELL_SRC, DEFAULT_ENVRC),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Try instantiating the trivial shell file we provide the user.
    #[test]
    fn trivial_shell_nix() -> std::io::Result<()> {
        let out = std::process::Command::new("nix-instantiate")
            // we can’t assume to have a <nixpkgs>, so use bogus-nixpkgs
            .args(&["-I", "nixpkgs=./nix/bogus-nixpkgs/"])
            .args(&["--expr", TRIVIAL_SHELL_SRC])
            .output()?;
        assert!(
            out.status.success(),
            "stdout:\n{}\nstderr:{}\n",
            std::str::from_utf8(&out.stdout).unwrap(),
            std::str::from_utf8(&out.stderr).unwrap()
        );
        Ok(())

        // TODO: provide .instantiate(), which does a plain nix-instantiate
        // and returns the .drv file.
        // let res = nix::CallOpts::expression(TRIVIAL_SHELL_SRC)
        //     .instantiate();

        // match res {
        //     Ok(_drv) => Ok(()),
        //     Err(nix::InstantiateError::ExecutionFailed(output)) =>
        //         panic!(
        //             "stdout:\n{}\nstderr:{}\n",
        //             std::str::from_utf8(&output.stdout).unwrap(),
        //             std::str::from_utf8(&output.stderr).unwrap()
        //         ),
        //     Err(nix::InstantiateError::Io(io)) => Err(io)
        // }
    }
}
