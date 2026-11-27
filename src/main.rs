use solia::{AppInstance, MAX_PACKAGE_BYTES, check_source, compile_source, run};

use std::{
    env, fs,
    io::{Read, Write},
    path::Path,
    process::ExitCode,
};

const HELP: &str = "Usage:\n  solia check <source.solia>\n  solia build <source.solia> [output.sapp]\n  solia run <source.solia | package.sapp>";

fn source<T>(path: &Path, process: impl FnOnce(&str) -> Result<T, String>) -> Result<T, String> {
    if !path.extension().is_some_and(|e| e == "solia") {
        return Err("expected a .solia source file".into());
    }
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    process(&text).map_err(|e| format!("{}:{e}", path.display()))
}

fn cli(args: &[String]) -> Result<(), String> {
    match args {
        [help] if help == "--help" || help == "-h" => {
            println!("{HELP}");
            Ok(())
        }
        [cmd, input] if cmd == "check" => {
            source(Path::new(input), check_source)?;
            println!("Checked {input}");
            Ok(())
        }
        [cmd, input, rest @ ..] if cmd == "build" && rest.len() <= 1 => {
            let path = Path::new(input);
            let out = rest
                .first()
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| path.with_extension("sapp"));
            if !out.extension().is_some_and(|e| e == "sapp") {
                return Err("expected .sapp output".into());
            }
            let bytes = source(path, compile_source)?;
            let mut tmp = tempfile::NamedTempFile::new_in(
                out.parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new(".")),
            )
            .map_err(|e| e.to_string())?;
            tmp.write_all(&bytes).map_err(|e| e.to_string())?;
            tmp.persist(&out)
                .map_err(|e| format!("{}: {e}", out.display()))?;
            println!("Built {}", out.display());
            Ok(())
        }
        [cmd, input] if cmd == "run" => {
            let path = Path::new(input);
            let bytes = if path.extension().is_some_and(|e| e == "sapp") {
                let mut buf = vec![];
                fs::File::open(path)
                    .and_then(|f| f.take((MAX_PACKAGE_BYTES + 1) as u64).read_to_end(&mut buf))
                    .map_err(|e| format!("{input}: {e}"))?;
                buf
            } else {
                source(path, compile_source)?
            };
            let mut app = AppInstance::load(&bytes)?;
            app.set_source(path.display().to_string());
            run(app)
        }
        _ => Err(HELP.into()),
    }
}

fn main() -> ExitCode {
    match cli(&env::args().skip(1).collect::<Vec<_>>()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
