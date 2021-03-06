// Copyright 2015 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#![cfg(not(test))]

#[macro_use]
extern crate log;
extern crate rustfmt;
extern crate toml;
extern crate env_logger;
extern crate getopts;

use rustfmt::{run, run_from_stdin};
use rustfmt::config::{Config, WriteMode};

use std::env;
use std::fs::{self, File};
use std::io::{self, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

use getopts::{Matches, Options};

/// Rustfmt operations.
enum Operation {
    /// Format files and their child modules.
    Format(Vec<PathBuf>, WriteMode),
    /// Print the help message.
    Help,
    // Print version information
    Version,
    /// Print detailed configuration help.
    ConfigHelp,
    /// Invalid program input, including reason.
    InvalidInput(String),
    /// No file specified, read from stdin
    Stdin(String, WriteMode),
}

/// Try to find a project file in the given directory and its parents. Returns the path of a the
/// nearest project file if one exists, or `None` if no project file was found.
fn lookup_project_file(dir: &Path) -> io::Result<Option<PathBuf>> {
    let mut current = if dir.is_relative() {
        try!(env::current_dir()).join(dir)
    } else {
        dir.to_path_buf()
    };

    current = try!(fs::canonicalize(current));

    loop {
        let config_file = current.join("rustfmt.toml");
        match fs::metadata(&config_file) {
            Ok(md) => {
                // Properly handle unlikely situation of a directory named `rustfmt.toml`.
                if md.is_file() {
                    return Ok(Some(config_file));
                }
            }
            // If it's not found, we continue searching; otherwise something went wrong and we
            // return the error.
            Err(e) => {
                if e.kind() != ErrorKind::NotFound {
                    return Err(e);
                }
            }
        }

        // If the current directory has no parent, we're done searching.
        if !current.pop() {
            return Ok(None);
        }
    }
}

/// Resolve the config for input in `dir`.
///
/// Returns the `Config` to use, and the path of the project file if there was
/// one.
fn resolve_config(dir: &Path) -> io::Result<(Config, Option<PathBuf>)> {
    let path = try!(lookup_project_file(dir));
    if path.is_none() {
        return Ok((Config::default(), None));
    }
    let path = path.unwrap();
    let mut file = try!(File::open(&path));
    let mut toml = String::new();
    try!(file.read_to_string(&mut toml));
    Ok((Config::from_toml(&toml), Some(path)))
}

fn update_config(config: &mut Config, matches: &Matches) {
    config.verbose = matches.opt_present("verbose");
    config.skip_children = matches.opt_present("skip-children");
}

fn execute() -> i32 {
    let mut opts = Options::new();
    opts.optflag("h", "help", "show this message");
    opts.optflag("V", "version", "show version information");
    opts.optflag("v", "verbose", "show progress");
    opts.optopt("",
                "write-mode",
                "mode to write in (not usable when piping from stdin)",
                "[replace|overwrite|display|diff|coverage|checkstyle]");
    opts.optflag("", "skip-children", "don't reformat child modules");

    opts.optflag("",
                 "config-help",
                 "show details of rustfmt configuration options");

    let matches = match opts.parse(env::args().skip(1)) {
        Ok(m) => m,
        Err(e) => {
            print_usage(&opts, &e.to_string());
            return 1;
        }
    };

    let operation = determine_operation(&matches);

    match operation {
        Operation::InvalidInput(reason) => {
            print_usage(&opts, &reason);
            1
        }
        Operation::Help => {
            print_usage(&opts, "");
            0
        }
        Operation::Version => {
            print_version();
            0
        }
        Operation::ConfigHelp => {
            Config::print_docs();
            0
        }
        Operation::Stdin(input, write_mode) => {
            // try to read config from local directory
            let (config, _) = resolve_config(&env::current_dir().unwrap())
                                  .expect("Error resolving config");

            run_from_stdin(input, write_mode, &config);
            0
        }
        Operation::Format(files, write_mode) => {
            for file in files {
                let (mut config, path) = resolve_config(file.parent().unwrap())
                                             .expect(&format!("Error resolving config for {}",
                                                              file.display()));
                if let Some(path) = path {
                    println!("Using rustfmt config file {} for {}",
                             path.display(),
                             file.display());
                }

                update_config(&mut config, &matches);
                run(&file, write_mode, &config);
            }
            0
        }
    }
}

fn main() {
    let _ = env_logger::init();
    let exit_code = execute();

    // Make sure standard output is flushed before we exit.
    std::io::stdout().flush().unwrap();

    // Exit with given exit code.
    //
    // NOTE: This immediately terminates the process without doing any cleanup,
    // so make sure to finish all necessary cleanup before this is called.
    std::process::exit(exit_code);
}

fn print_usage(opts: &Options, reason: &str) {
    let reason = format!("{}\nusage: {} [options] <file>...",
                         reason,
                         env::args_os().next().unwrap().to_string_lossy());
    println!("{}", opts.usage(&reason));
}

fn print_version() {
    println!("{}.{}.{}{}",
             option_env!("CARGO_PKG_VERSION_MAJOR").unwrap_or("X"),
             option_env!("CARGO_PKG_VERSION_MINOR").unwrap_or("X"),
             option_env!("CARGO_PKG_VERSION_PATCH").unwrap_or("X"),
             option_env!("CARGO_PKG_VERSION_PRE").unwrap_or(""));
}

fn determine_operation(matches: &Matches) -> Operation {
    if matches.opt_present("h") {
        return Operation::Help;
    }

    if matches.opt_present("config-help") {
        return Operation::ConfigHelp;
    }

    if matches.opt_present("version") {
        return Operation::Version;
    }

    // if no file argument is supplied, read from stdin
    if matches.free.is_empty() {

        let mut buffer = String::new();
        match io::stdin().read_to_string(&mut buffer) {
            Ok(..) => (),
            Err(e) => return Operation::InvalidInput(e.to_string()),
        }

        // WriteMode is always plain for Stdin
        return Operation::Stdin(buffer, WriteMode::Plain);
    }

    let write_mode = match matches.opt_str("write-mode") {
        Some(mode) => {
            match mode.parse() {
                Ok(mode) => mode,
                Err(..) => return Operation::InvalidInput("Unrecognized write mode".into()),
            }
        }
        None => WriteMode::Default,
    };

    let files: Vec<_> = matches.free.iter().map(PathBuf::from).collect();

    Operation::Format(files, write_mode)
}
