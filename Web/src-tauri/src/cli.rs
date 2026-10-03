//! The converter on the command line, for scripts and for converting many
//! parts at once. It makes the same files as the window, the same way.
//! The app runs this when it is started with part numbers or options.

use std::path::PathBuf;
use std::process::ExitCode;

use crate::convert::{self, LibraryMode, Report, Request};
use crate::easyeda::api::Client;
use crate::lock;
use crate::messages::{Message, Problem, Surface};
use serde::Serialize;

const HELP: &str = "\
Usage: EasyEDA-to-KiCad-Converter <part>... [options]

Converts LCSC parts to KiCad symbols, footprints, and 3D models. When the
output folder is in a KiCad project, each library is added to the project's
library tables, so its parts are ready to place. Started with no part and no
option, it opens the window.

A part is its LCSC number, like C25804.

Options:
  -o, --output <folder>   The folder the libraries go in. Default: the
                          current folder.
  -l, --library <name>    Put every part in one library with this name.
                          Default: a folder of its own for each part, named
                          after it.
      --only <items>      Write only these of symbol, footprint, and model,
                          separated by commas, like --only symbol,footprint.
      --overwrite         Replace a symbol, footprint, or 3D model the
                          library already has.
      --full-model-paths  Outside a KiCad project, point each footprint at
                          its 3D model by full path, rather than through
                          ${KIPRJMOD} with the output folder taken as the
                          project folder.
      --json              Print the result as JSON.
  -h, --help              Show this help.
  -V, --version           Show the version.

Exits with 0 when every part converted, 1 when any did not, and 2 when the
command itself is wrong.
";

/// What to convert, read from the command line.
struct Run {
    parts: Vec<String>,
    output: PathBuf,
    library: Option<String>,
    symbol: bool,
    footprint: bool,
    model: bool,
    overwrite: bool,
    full_model_paths: bool,
    json: bool,
}

enum Command {
    Convert(Run),
    Help,
    Version,
}

fn parse(args: Vec<String>) -> Result<Command, String> {
    let mut run = Run {
        parts: Vec::new(),
        output: PathBuf::new(),
        library: None,
        symbol: true,
        footprint: true,
        model: true,
        overwrite: false,
        full_model_paths: false,
        json: false,
    };
    let mut output = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        // `--option value` and `--option=value` both work.
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) if name.starts_with("--") => {
                (name.to_string(), Some(value.to_string()))
            }
            _ => (arg.clone(), None),
        };
        let switch = matches!(
            name.as_str(),
            "--help" | "--version" | "--overwrite" | "--full-model-paths" | "--json"
        );
        if switch && inline.is_some() {
            return Err(format!("{name} takes no value."));
        }
        let mut value = |option: &str| {
            inline
                .clone()
                .or_else(|| args.next())
                .ok_or_else(|| format!("{option} needs a value."))
        };
        match name.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "-o" | "--output" => output = Some(PathBuf::from(value("--output")?)),
            "-l" | "--library" => {
                let library = value("--library")?;
                if library.trim().is_empty() {
                    return Err("--library needs a name.".into());
                }
                run.library = Some(library);
            }
            "--only" => {
                let items = value("--only")?;
                run.symbol = false;
                run.footprint = false;
                run.model = false;
                for item in items.split(',').map(str::trim) {
                    match item {
                        "symbol" => run.symbol = true,
                        "footprint" => run.footprint = true,
                        "model" => run.model = true,
                        other => {
                            return Err(format!(
                                "--only takes symbol, footprint, and model, not {other:?}."
                            ));
                        }
                    }
                }
            }
            "--overwrite" => run.overwrite = true,
            "--full-model-paths" => run.full_model_paths = true,
            "--json" => run.json = true,
            _ if inline.is_some() || name.starts_with('-') => {
                return Err(format!("There is no option {name}."));
            }
            part => match convert::normalize_part_number(part) {
                Some(part) if !run.parts.contains(&part) => run.parts.push(part),
                Some(_) => {}
                None => {
                    return Err(format!(
                        "{part} is not an LCSC part number. One is the letter C followed by digits, like C25804."
                    ));
                }
            },
        }
    }
    if run.parts.is_empty() {
        return Err("Name at least one part, like C25804.".into());
    }
    // A full path, so every saved file is reported the same way.
    run.output = std::path::absolute(output.unwrap_or_else(|| PathBuf::from(".")))
        .map_err(|error| format!("Could not read the output folder: {error}"))?;
    Ok(Command::Convert(run))
}

/// One part's outcome, as `--json` prints it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Outcome {
    lcsc_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    report: Option<Report>,
    #[serde(skip_serializing_if = "Option::is_none")]
    problem: Option<Message>,
}

async fn convert_part(run: &Run, client: &Client, lcsc_id: &str) -> Result<Report, Problem> {
    let component = client
        .component(lcsc_id)
        .await
        .map_err(|error| Problem::from_fetch(error, lcsc_id))?;
    let (mode, library_name) = match &run.library {
        Some(name) => (LibraryMode::CustomLibrary, name.clone()),
        None => (
            LibraryMode::SinglePart,
            convert::summarize(lcsc_id, &component).suggested_library_name,
        ),
    };
    let request = Request {
        lcsc_id: lcsc_id.to_string(),
        output_folder: run.output.to_string_lossy().into_owned(),
        mode,
        library_name,
        symbol: run.symbol,
        footprint: run.footprint,
        model: run.model,
        overwrite: run.overwrite,
        project_relative: !run.full_model_paths,
    };
    convert::convert(&request, &component, client).await
}

fn print_report(lcsc_id: &str, report: &Report) {
    println!(
        "{lcsc_id}: converted into {}",
        report.destination.library_name
    );
    for path in &report.written {
        println!("  saved {path}");
    }
    for table in &report.tables {
        println!("  added to {table}");
    }
    if let Some(symbol) = &report.symbol_id {
        println!("  Add it in the schematic as {symbol}.");
    }
    for note in &report.notes {
        println!("  {note}");
    }
}

fn print_problem(lcsc_id: &str, message: &Message) {
    eprintln!("{lcsc_id}: not converted. {}", message.text);
    if let Some(detail) = &message.detail {
        eprintln!("  {detail}");
    }
}

async fn convert_all(run: &Run) -> Result<Vec<Outcome>, Problem> {
    // Held for the whole run, so the window or another run never rewrites
    // the same library or library table meanwhile.
    let _lock = lock::hold()?;
    let client = Client::new();
    let mut outcomes = Vec::new();
    for lcsc_id in &run.parts {
        let outcome = match convert_part(run, &client, lcsc_id).await {
            Ok(report) => {
                if !run.json {
                    print_report(lcsc_id, &report);
                }
                Outcome {
                    lcsc_id: lcsc_id.clone(),
                    report: Some(report),
                    problem: None,
                }
            }
            Err(problem) => {
                let message = problem.describe(Surface::CommandLine);
                if !run.json {
                    print_problem(lcsc_id, &message);
                }
                Outcome {
                    lcsc_id: lcsc_id.clone(),
                    report: None,
                    problem: Some(message),
                }
            }
        };
        outcomes.push(outcome);
    }
    Ok(outcomes)
}

/// Runs the command line with the arguments after the program's name, and
/// returns what it exits with: 0 when every part converted, 1 when any did
/// not, and 2 when the command itself is wrong.
pub fn run(args: Vec<String>) -> ExitCode {
    let run = match parse(args) {
        Ok(Command::Convert(run)) => run,
        Ok(Command::Help) => {
            print!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Ok(Command::Version) => {
            println!("EasyEDA to KiCad Converter {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("{message}\nRun EasyEDA-to-KiCad-Converter --help for how to use it.");
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Could not start: {error}");
            return ExitCode::FAILURE;
        }
    };
    let outcomes = match runtime.block_on(convert_all(&run)) {
        Ok(outcomes) => outcomes,
        Err(problem) => {
            let message = problem.describe(Surface::CommandLine);
            eprintln!("{}", message.text);
            if let Some(detail) = &message.detail {
                eprintln!("  {detail}");
            }
            return ExitCode::FAILURE;
        }
    };
    if run.json {
        let json = serde_json::json!({ "parts": outcomes });
        println!("{json:#}");
    }
    if outcomes.iter().all(|outcome| outcome.report.is_some()) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_run(args: &[&str]) -> Result<Run, String> {
        match parse(args.iter().map(|arg| arg.to_string()).collect())? {
            Command::Convert(run) => Ok(run),
            Command::Help | Command::Version => Err("not a conversion".into()),
        }
    }

    #[test]
    fn options_are_read() {
        let run = parse_run(&[
            "c25804",
            "C2040",
            "C25804",
            "--output=lib",
            "-l",
            "My Parts",
            "--only",
            "symbol, footprint",
            "--overwrite",
            "--json",
        ])
        .unwrap();
        assert_eq!(run.parts, ["C25804", "C2040"]);
        assert!(run.output.is_absolute() && run.output.ends_with("lib"));
        assert_eq!(run.library.as_deref(), Some("My Parts"));
        assert!(run.symbol && run.footprint && !run.model);
        assert!(run.overwrite && run.json && !run.full_model_paths);

        let defaults = parse_run(&["C1"]).unwrap();
        assert_eq!(defaults.output, std::env::current_dir().unwrap());
        assert!(defaults.symbol && defaults.footprint && defaults.model);
        assert!(!defaults.overwrite && defaults.library.is_none());
    }

    #[test]
    fn mistakes_are_named() {
        for (args, expected) in [
            (&[][..], "Name at least one part"),
            (&["25804"][..], "25804 is not an LCSC part number"),
            (&["C1", "--only", "symbol,3d"][..], "not \"3d\""),
            (&["C1", "--output"][..], "--output needs a value"),
            (&["C1", "--library", " "][..], "--library needs a name"),
            (&["C1", "--force"][..], "There is no option --force"),
            (&["C1", "--json=yes"][..], "--json takes no value"),
            (&["C1", "--colour=red"][..], "There is no option --colour"),
        ] {
            let error = parse_run(args).err().unwrap();
            assert!(error.contains(expected), "{args:?}: {error}");
        }
        assert!(matches!(parse(vec!["-h".into()]), Ok(Command::Help)));
    }
}
