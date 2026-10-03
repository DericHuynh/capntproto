use capntproto_compiler::FileCompiler;
use std::io::Write;
use std::path::PathBuf;

const USAGE: &str = "usage: capntproto-compile [-I DIR] [--src-prefix DIR] FILE.capnp... > request.bin\nEmits a CodeGeneratorRequest for the supported schema language.\n-I, --import-path DIR  Search directory for imports beginning with '/'.\n--src-prefix DIR      Strip this directory from requested filenames (default: working directory).";

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut compiler = FileCompiler::new();
    let mut files = Vec::new();
    let mut args = std::env::args_os().skip(1);
    let mut positional = false;
    while let Some(argument) = args.next() {
        let text = argument.to_str();
        if !positional {
            match text {
                Some("--") => {
                    positional = true;
                    continue;
                }
                Some("--help" | "-h") => {
                    println!("{USAGE}");
                    return Ok(());
                }
                Some("-I" | "--import-path") => {
                    compiler.import_path(args.next().ok_or("missing import directory")?);
                    continue;
                }
                Some("--src-prefix") => {
                    compiler.src_prefix(args.next().ok_or("missing source prefix")?);
                    continue;
                }
                Some(value) if value.starts_with("--import-path=") => {
                    compiler.import_path(&value[14..]);
                    continue;
                }
                Some(value) if value.starts_with("--src-prefix=") => {
                    compiler.src_prefix(&value[13..]);
                    continue;
                }
                Some(value) if value.starts_with("-I") => {
                    compiler.import_path(&value[2..]);
                    continue;
                }
                Some(value) if value.starts_with('-') => {
                    return Err(format!("unknown option: {value}\n{USAGE}").into())
                }
                _ => (),
            }
        }
        files.push(PathBuf::from(argument));
    }
    if files.is_empty() {
        return Err(USAGE.into());
    }
    let message = compiler.compile(&files)?;
    let mut stdout = std::io::stdout().lock();
    capnp::serialize::write_message(&mut stdout, &message)?;
    stdout.flush()?;
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
