use std::env;
use std::process::ExitCode;

#[derive(Debug, Clone)]
pub struct CliArgs {
    pub rom_path: String,
    pub bios_path: Option<String>,
    pub frames: Option<u32>,
    pub debug_interval: Option<u32>,
    pub stuck_threshold: Option<u32>,
    pub bios_watchdog: Option<u32>,
    pub trace_branches: bool,
}

fn parse_u32_arg(bin: &str, args: &mut env::Args, flag: &str) -> Result<u32, ExitCode> {
    let Some(value) = args.next() else {
        eprintln!("Missing value for {flag}");
        print_usage(bin);
        return Err(ExitCode::from(2));
    };

    match value.parse::<u32>() {
        Ok(parsed) => Ok(parsed),
        Err(_) => {
            eprintln!("Invalid {flag} value: {value}");
            print_usage(bin);
            Err(ExitCode::from(2))
        }
    }
}

pub fn print_usage(bin: &str) {
    eprintln!(
        "Usage: {bin} --rom <path> [--bios <path>] [--frames <n>] [--debug-interval <frames>] [--stuck-threshold <frames>] [--bios-watchdog <frames>] [--trace-branches]"
    );
}

pub fn parse_args() -> Result<CliArgs, ExitCode> {
    let mut args = env::args();
    let bin = args.next().unwrap_or_else(|| "gba-emu".to_string());

    let mut rom_path: Option<String> = None;
    let mut bios_path: Option<String> = None;
    let mut frames: Option<u32> = None;
    let mut debug_interval: Option<u32> = None;
    let mut stuck_threshold: Option<u32> = None;
    let mut bios_watchdog: Option<u32> = None;
    let mut trace_branches = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--rom" => {
                rom_path = args.next();
                if rom_path.is_none() {
                    eprintln!("Missing value for --rom");
                    print_usage(&bin);
                    return Err(ExitCode::from(2));
                }
            }
            "--bios" => {
                bios_path = args.next();
                if bios_path.is_none() {
                    eprintln!("Missing value for --bios");
                    print_usage(&bin);
                    return Err(ExitCode::from(2));
                }
            }
            "--frames" => frames = Some(parse_u32_arg(&bin, &mut args, "--frames")?),
            "--debug-interval" => {
                debug_interval = Some(parse_u32_arg(&bin, &mut args, "--debug-interval")?)
            }
            "--stuck-threshold" => {
                stuck_threshold = Some(parse_u32_arg(&bin, &mut args, "--stuck-threshold")?)
            }
            "--bios-watchdog" => {
                bios_watchdog = Some(parse_u32_arg(&bin, &mut args, "--bios-watchdog")?)
            }
            "--trace-branches" => {
                trace_branches = true;
            }
            "-h" | "--help" => {
                print_usage(&bin);
                return Err(ExitCode::SUCCESS);
            }
            _ => {
                eprintln!("Unknown argument: {arg}");
                print_usage(&bin);
                return Err(ExitCode::from(2));
            }
        }
    }

    let Some(rom_path) = rom_path else {
        print_usage(&bin);
        return Err(ExitCode::from(2));
    };

    Ok(CliArgs {
        rom_path,
        bios_path,
        frames,
        debug_interval,
        stuck_threshold,
        bios_watchdog,
        trace_branches,
    })
}
