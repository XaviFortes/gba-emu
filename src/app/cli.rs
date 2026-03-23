use std::env;
use std::process::ExitCode;

#[derive(Debug, Clone)]
pub struct CliArgs {
    pub rom_path: Option<String>,
    pub bios_path: Option<String>,
    pub speed_multiplier: Option<u32>,
    pub frames: Option<u32>,
    pub debug_interval: Option<u32>,
    pub stuck_threshold: Option<u32>,
    pub bios_watchdog: Option<u32>,
    pub trace_branches: bool,
    pub launcher: bool,
    pub roms_dir: Option<String>,
    pub window_scale: Option<u32>,
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
        "Usage: {bin} [--rom <path>] [--bios <path>] [--speed <1..3>] [--frames <n>] [--debug-interval <frames>] [--stuck-threshold <frames>] [--bios-watchdog <frames>] [--trace-branches] [--launcher] [--roms-dir <dir>] [--scale <2..6>]"
    );
}

pub fn parse_args() -> Result<CliArgs, ExitCode> {
    let mut args = env::args();
    let bin = args.next().unwrap_or_else(|| "gba-emu".to_string());

    let mut rom_path: Option<String> = None;
    let mut bios_path: Option<String> = None;
    let mut speed_multiplier: Option<u32> = None;
    let mut frames: Option<u32> = None;
    let mut debug_interval: Option<u32> = None;
    let mut stuck_threshold: Option<u32> = None;
    let mut bios_watchdog: Option<u32> = None;
    let mut trace_branches = false;
    let mut launcher = false;
    let mut roms_dir: Option<String> = None;
    let mut window_scale: Option<u32> = None;

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
            "--speed" => {
                let speed = parse_u32_arg(&bin, &mut args, "--speed")?;
                if !(1..=3).contains(&speed) {
                    eprintln!("Invalid --speed value: {speed}. Expected range 1..=3");
                    print_usage(&bin);
                    return Err(ExitCode::from(2));
                }
                speed_multiplier = Some(speed);
            }
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
            "--launcher" => {
                launcher = true;
            }
            "--roms-dir" => {
                roms_dir = args.next();
                if roms_dir.is_none() {
                    eprintln!("Missing value for --roms-dir");
                    print_usage(&bin);
                    return Err(ExitCode::from(2));
                }
            }
            "--scale" => {
                let scale = parse_u32_arg(&bin, &mut args, "--scale")?;
                if !(1..=6).contains(&scale) {
                    eprintln!("Invalid --scale value: {scale}. Expected range 1..=6");
                    print_usage(&bin);
                    return Err(ExitCode::from(2));
                }
                window_scale = Some(scale);
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

    if rom_path.is_none() {
        launcher = true;
    }

    Ok(CliArgs {
        rom_path,
        bios_path,
        speed_multiplier,
        frames,
        debug_interval,
        stuck_threshold,
        bios_watchdog,
        trace_branches,
        launcher,
        roms_dir,
        window_scale,
    })
}
