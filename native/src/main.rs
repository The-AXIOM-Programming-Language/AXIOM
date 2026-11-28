pub mod eeg;
pub mod edf;
pub mod edf_data;
pub mod units;
pub mod simulation;
pub mod language;

use std::{env, fs, path::Path, process};

fn print_help() {
    println!("AXIOM — scientific programming language");
    println!("\nUSAGE:");
    println!("  axiom <COMMAND> [OPTIONS]");
    println!("\nCOMMANDS:");
    println!("  run <FILE.ax>       Execute an AXIOM program");
    println!("  check <FILE.ax>     Validate AXIOM source syntax");
    println!("  new <NAME>          Create a starter AXIOM program");
    println!("  doctor              Check the native toolchain environment");
    println!("  build <FILE.ax>     Check source and report backend status");
    println!("  help                Show this help");
    println!("\nOPTIONS:");
    println!("  -h, --help          Show help");
    println!("  -V, --version       Show version");
}
fn read_ax_file(path: &str) -> Result<String, String> {
    if !path.ends_with(".ax") { return Err("source files must use the .ax extension".into()); }
    fs::read_to_string(path).map_err(|e| format!("cannot read '{path}': {e}"))
}
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() || args[0] == "help" || args[0] == "--help" || args[0] == "-h" {
        print_help(); return;
    }
    if args[0] == "--version" || args[0] == "-V" || args[0] == "version" {
        println!("AXIOM {}", env!("CARGO_PKG_VERSION")); return;
    }
    let fail = |message: String, code: i32| -> ! {
        eprintln!("error: {message}");
        process::exit(code);
    };
    match args[0].as_str() {
        "check" | "run" | "build" => {
            if args.len() != 2 { fail(format!("usage: axiom {} <FILE.ax>", args[0]), 2); }
            let path = &args[1];
            let source = read_ax_file(path).unwrap_or_else(|e| fail(e, 2));
            match language::check_source(&source) {
                Ok(()) => {},
                Err(e) => fail(format!("{path}: {e}"), 1),
            }
            match args[0].as_str() {
                "check" => println!("✓ {path}: syntax OK"),
                "run" => match language::run_source(&source) {
                    Ok(lines) => for line in lines { println!("{line}"); },
                    Err(e) => fail(format!("{path}: {e}"), 1),
                },
                "build" => {
                    println!("✓ {path}: source validation passed");
                    println!("AXIOM native code generation is not available yet.");
                    println!("Use 'axiom run {path}' to execute with the native interpreter.");
                },
                _ => unreachable!(),
            }
        }
        "new" => {
            if args.len() != 2 { fail("usage: axiom new <NAME>".into(), 2); }
            let name = &args[1];
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                fail("project name may contain only letters, numbers, '-' and '_'".into(), 2);
            }
            let path = format!("{name}.ax");
            if Path::new(&path).exists() { fail(format!("'{path}' already exists"), 1); }
            let template = format!("# {name}\n# An AXIOM computational neuroscience experiment\n\nneuron \"{name}\":\n    membrane_time = 20 ms\n    resting_voltage = -65 mV\n    spike_threshold = -50 mV\n    reset_voltage = -65 mV\n    input_resistance = 10 MOhm\n    initial_voltage = -65 mV\n\ninject current 2 nA\nsimulate for 100 ms using time_step 0.1 ms using solver rk4\n\nshow spike_count\nshow spike_times\nsave results to \"{name}_results.csv\"\n");
            fs::write(&path, template).unwrap_or_else(|e| fail(format!("cannot create '{path}': {e}"), 2));
            println!("Created {path}");
            println!("Next: axiom check {path}");
        }
        "doctor" => {
            println!("AXIOM {}", env!("CARGO_PKG_VERSION"));
            println!("Native Rust runtime: available");
            println!("EEG/EDF modules: included in this build");
            println!("Native machine-code backend: not implemented");
        }
        other => fail(format!("unknown command '{other}'. Run 'axiom help'."), 2),
    }
}

