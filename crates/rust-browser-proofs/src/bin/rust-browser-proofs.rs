//! Run a consumer-selected test command with the Rustup toolchain environment.

use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::{self, Command, ExitCode},
    time::{SystemTime, UNIX_EPOCH},
};

fn main() -> ExitCode {
    match run(env::args_os().skip(1).collect()) {
        Ok(status) => ExitCode::from(status),
        Err(message) => {
            eprintln!("rust-browser-proofs: {message}");
            ExitCode::from(2)
        }
    }
}

fn run(arguments: Vec<OsString>) -> Result<u8, String> {
    let invocation = Invocation::parse(arguments)?;
    let environment = EnvironmentSnapshot::collect();
    let command_result = if let Some(command) = &invocation.command {
        reject_virtual_workspace_wasm_pack(command)?;
        let toolchain = rustup_toolchain()?;
        let status = Command::new(&command.program)
            .args(&command.arguments)
            .env("RUSTC", toolchain.join("rustc"))
            .env("CARGO", toolchain.join("cargo"))
            .env("PATH", prepend_path(&toolchain)?)
            .status()
            .map_err(|error| format!("could not start {:?}: {error}", command.program))?;
        Some(CommandResult::from_status(status))
    } else {
        None
    };

    if let Some(report_path) = &invocation.report_path {
        write_report(
            report_path,
            &render_report(&environment, &invocation, command_result.as_ref()),
        )?;
        println!(
            "rust-browser-proofs: wrote Markdown report to {}",
            report_path.display()
        );
    }

    Ok(command_result.map_or(0, |result| result.exit_code))
}

fn reject_virtual_workspace_wasm_pack(command: &CommandInvocation) -> Result<(), String> {
    let current_dir = env::current_dir()
        .map_err(|error| format!("could not determine the current directory: {error}"))?;
    let manifest = match fs::read_to_string(current_dir.join("Cargo.toml")) {
        Ok(manifest) => manifest,
        Err(_) => return Ok(()),
    };
    let Some(message) =
        virtual_workspace_wasm_pack_error(&command.program, &command.arguments, &manifest)
    else {
        return Ok(());
    };

    let fixture_hint = if current_dir
        .join("fixtures/consumer-battery/Cargo.toml")
        .is_file()
    {
        " For this checkout: `cd fixtures/consumer-battery` first."
    } else {
        ""
    };
    Err(format!("{message}{fixture_hint}"))
}

fn virtual_workspace_wasm_pack_error(
    program: &OsStr,
    arguments: &[OsString],
    manifest: &str,
) -> Option<&'static str> {
    let has_manifest_path = arguments.iter().any(|argument| {
        argument == "--manifest-path" || argument.to_string_lossy().starts_with("--manifest-path=")
    });
    if program != OsStr::new("wasm-pack")
        || has_manifest_path
        || !has_manifest_table(manifest, "workspace")
        || has_manifest_table(manifest, "package")
    {
        return None;
    }

    Some(
        "`wasm-pack` must run from a Cargo package directory, but the current directory is a virtual workspace.",
    )
}

fn has_manifest_table(manifest: &str, table: &str) -> bool {
    manifest
        .lines()
        .any(|line| line.trim() == format!("[{table}]"))
}

fn rustup_toolchain() -> Result<PathBuf, String> {
    let output = Command::new("rustup")
        .args(["which", "rustc"])
        .output()
        .map_err(|error| {
            format!("rustup is required to select the wasm-capable toolchain: {error}")
        })?;
    if !output.status.success() {
        return Err(format!(
            "rustup could not select rustc: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let rustc = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let toolchain = rustc
        .parent()
        .filter(|path| path.join("cargo").is_file())
        .ok_or_else(|| {
            format!(
                "rustup returned an incomplete toolchain path: {}",
                rustc.display()
            )
        })?;
    Ok(toolchain.to_path_buf())
}

fn prepend_path(toolchain: &Path) -> Result<OsString, String> {
    let inherited = env::var_os("PATH").unwrap_or_default();
    let paths = std::iter::once(toolchain.to_path_buf()).chain(env::split_paths(&inherited));
    env::join_paths(paths).map_err(|error| format!("could not construct PATH: {error}"))
}

struct CommandResult {
    exit_code: u8,
}

impl CommandResult {
    fn from_status(status: process::ExitStatus) -> Self {
        Self {
            exit_code: status.code().unwrap_or(1) as u8,
        }
    }

    fn label(&self) -> &'static str {
        if self.exit_code == 0 {
            "passed"
        } else {
            "failed"
        }
    }
}

struct Probe {
    status: &'static str,
    detail: String,
}

struct EnvironmentSnapshot {
    rustup: Probe,
    wasm_target: Probe,
    wasm_pack: Probe,
    chrome: Probe,
    firefox: Probe,
    safari: Probe,
    edge: Probe,
    chromedriver: Probe,
    android: Probe,
    ios_simulator: Probe,
}

impl EnvironmentSnapshot {
    fn collect() -> Self {
        Self {
            rustup: rustup_probe(),
            wasm_target: wasm_target_probe(),
            wasm_pack: executable_probe(&["wasm-pack"], &[], "wasm-pack"),
            chrome: executable_probe(
                &[
                    "google-chrome",
                    "google-chrome-stable",
                    "chromium",
                    "chromium-browser",
                ],
                &["/Applications/Google Chrome.app"],
                "Chrome or Chromium",
            ),
            firefox: executable_probe(&["firefox"], &["/Applications/Firefox.app"], "Firefox"),
            safari: safari_probe(),
            edge: executable_probe(
                &["microsoft-edge", "microsoft-edge-stable", "msedge"],
                &["/Applications/Microsoft Edge.app"],
                "Microsoft Edge",
            ),
            chromedriver: executable_probe(
                &["chromedriver"],
                &[".tools/chromedriver"],
                "ChromeDriver",
            ),
            android: android_probe(),
            ios_simulator: ios_simulator_probe(),
        }
    }
}

fn rustup_probe() -> Probe {
    match Command::new("rustup").args(["which", "rustc"]).output() {
        Ok(output) if output.status.success() => Probe {
            status: "available",
            detail: format!("rustc at {}", output_text(&output)),
        },
        Ok(output) => Probe {
            status: "missing",
            detail: format!("rustup failed: {}", output_text(&output)),
        },
        Err(error) => Probe {
            status: "missing",
            detail: format!("rustup is unavailable: {error}"),
        },
    }
}

fn wasm_target_probe() -> Probe {
    match Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
    {
        Ok(output)
            if output.status.success()
                && output_text(&output)
                    .lines()
                    .any(|target| target == "wasm32-unknown-unknown") =>
        {
            Probe {
                status: "available",
                detail: "wasm32-unknown-unknown is installed".to_owned(),
            }
        }
        Ok(output) if output.status.success() => Probe {
            status: "missing",
            detail: "wasm32-unknown-unknown is not installed for rustup".to_owned(),
        },
        Ok(output) => Probe {
            status: "missing",
            detail: format!("could not list rustup targets: {}", output_text(&output)),
        },
        Err(error) => Probe {
            status: "missing",
            detail: format!("rustup is unavailable: {error}"),
        },
    }
}

fn executable_probe(programs: &[&str], paths: &[&str], label: &str) -> Probe {
    match find_executable(programs, paths) {
        Some(location) => Probe {
            status: "available",
            detail: format!("detected at {location}"),
        },
        None => Probe {
            status: "missing",
            detail: format!("{label} was not found on PATH or at an expected host path"),
        },
    }
}

fn safari_probe() -> Probe {
    if !cfg!(target_os = "macos") {
        return Probe {
            status: "not applicable",
            detail: "Safari WebDriver is only probed on macOS".to_owned(),
        };
    }
    executable_probe(
        &["safaridriver"],
        &["/Applications/Safari.app", "/usr/bin/safaridriver"],
        "Safari and SafariDriver",
    )
}

fn android_probe() -> Probe {
    let Some(adb) = find_executable(&["adb"], &[]) else {
        return Probe {
            status: "missing",
            detail: "Android Debug Bridge was not found on PATH".to_owned(),
        };
    };
    match Command::new(&adb).arg("devices").output() {
        Ok(output) if output.status.success() => {
            let device_output = output_text(&output);
            let devices: Vec<String> = device_output
                .lines()
                .skip(1)
                .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
                .collect();
            let detail = if devices.is_empty() {
                format!("adb at {adb}; no attached device or booted emulator")
            } else {
                format!("adb at {adb}; attached targets: {}", devices.join(", "))
            };
            Probe {
                status: "available",
                detail,
            }
        }
        Ok(output) => Probe {
            status: "missing",
            detail: format!("adb at {adb} failed: {}", output_text(&output)),
        },
        Err(error) => Probe {
            status: "missing",
            detail: format!("could not run adb at {adb}: {error}"),
        },
    }
}

fn ios_simulator_probe() -> Probe {
    if !cfg!(target_os = "macos") {
        return Probe {
            status: "not applicable",
            detail: "the iOS simulator is only probed on macOS".to_owned(),
        };
    }
    let Some(xcrun) = find_executable(&["xcrun"], &[]) else {
        return Probe {
            status: "missing",
            detail: "xcrun was not found on PATH".to_owned(),
        };
    };
    match Command::new(&xcrun)
        .args(["simctl", "list", "devices", "booted"])
        .output()
    {
        Ok(output) if output.status.success() => {
            let detail = if output_text(&output).contains("Booted") {
                format!("xcrun at {xcrun}; a simulator is booted")
            } else {
                format!("xcrun at {xcrun}; no simulator is booted")
            };
            Probe {
                status: "available",
                detail,
            }
        }
        Ok(output) => Probe {
            status: "missing",
            detail: format!("xcrun at {xcrun} failed: {}", output_text(&output)),
        },
        Err(error) => Probe {
            status: "missing",
            detail: format!("could not run xcrun at {xcrun}: {error}"),
        },
    }
}

fn find_executable(programs: &[&str], paths: &[&str]) -> Option<String> {
    for path in paths {
        let path = Path::new(path);
        if path.is_file() || path.is_dir() {
            return Some(path.display().to_string());
        }
    }
    let search_paths = env::var_os("PATH")?;
    for program in programs {
        for path in env::split_paths(&search_paths) {
            let candidate = path.join(program);
            if candidate.is_file() {
                return Some(candidate.display().to_string());
            }
        }
    }
    None
}

fn output_text(output: &process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if stdout.is_empty() {
        String::from_utf8_lossy(&output.stderr).trim().to_owned()
    } else {
        stdout
    }
}

fn write_report(path: &Path, report: &str) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "could not create report directory {}: {error}",
                parent.display()
            )
        })?;
    }
    fs::write(path, report)
        .map_err(|error| format!("could not write report {}: {error}", path.display()))
}

fn render_report(
    environment: &EnvironmentSnapshot,
    invocation: &Invocation,
    command_result: Option<&CommandResult>,
) -> String {
    let generated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let command = invocation
        .command
        .as_ref()
        .map(format_command)
        .unwrap_or_else(|| "No command requested; host capability report only.".to_owned());
    let command_status = command_result.map_or_else(
        || "not run".to_owned(),
        |result| format!("{} (exit {})", result.label(), result.exit_code),
    );
    let chrome_test = browser_test_status(invocation, command_result, "--chrome");
    let firefox_test = browser_test_status(invocation, command_result, "--firefox");
    let safari_test = browser_test_status(invocation, command_result, "--safari");

    format!(
        "# Rust Browser Proof Environment Report\n\n\
Generated at Unix timestamp `{generated_at}`.\n\n\
## Invocation\n\n\
- Command: {command}\n\
- Result: {command_status}\n\n\
## Toolchain\n\n\
| Check | Host status | Detail |\n|---|---|---|\n\
| Rustup toolchain | {} | {} |\n\
| wasm32 target | {} | {} |\n\
| wasm-pack | {} | {} |\n\n\
## Browser And Device Status\n\n\
| Target | Host prerequisite status | Detail | Test evidence from this invocation |\n|---|---|---|---|\n\
| Desktop Chrome or Chromium | {} | {} | {chrome_test} |\n\
| Desktop Firefox | {} | {} | {firefox_test} |\n\
| Desktop Safari or WebKit | {} | {} | {safari_test} |\n\
| Microsoft Edge | {} | {} | Not exercised by this runner invocation |\n\
| ChromeDriver | {} | {} | Driver presence is not a browser test result |\n\
| Android Chrome | {} | {} | Not exercised by this runner invocation |\n\
| iPhone Safari | {} | {} | Not exercised by this runner invocation |\n\
| iPhone Chrome | {} | {} | Not exercised by this runner invocation |\n\n\
## Interpretation\n\n\
- `available` means the local prerequisite probe succeeded; it does not prove storage behavior.\n\
- A browser is `passed` only when this exact invocation was an explicit `wasm-pack` run for that browser and exited successfully.\n\
- Mobile and Edge need their named runner/device workflow before they can claim execution coverage.\n",
        environment.rustup.status,
        markdown_cell(&environment.rustup.detail),
        environment.wasm_target.status,
        markdown_cell(&environment.wasm_target.detail),
        environment.wasm_pack.status,
        markdown_cell(&environment.wasm_pack.detail),
        environment.chrome.status,
        markdown_cell(&environment.chrome.detail),
        environment.firefox.status,
        markdown_cell(&environment.firefox.detail),
        environment.safari.status,
        markdown_cell(&environment.safari.detail),
        environment.edge.status,
        markdown_cell(&environment.edge.detail),
        environment.chromedriver.status,
        markdown_cell(&environment.chromedriver.detail),
        environment.android.status,
        markdown_cell(&environment.android.detail),
        environment.ios_simulator.status,
        markdown_cell(&environment.ios_simulator.detail),
        environment.ios_simulator.status,
        markdown_cell(&environment.ios_simulator.detail),
    )
}

fn browser_test_status(
    invocation: &Invocation,
    command_result: Option<&CommandResult>,
    browser_flag: &str,
) -> String {
    let Some(command) = &invocation.command else {
        return "not run".to_owned();
    };
    if command.program != OsStr::new("wasm-pack")
        || !command
            .arguments
            .iter()
            .any(|argument| argument == browser_flag)
    {
        return "not identified by this command".to_owned();
    }
    command_result.map_or_else(
        || "not run".to_owned(),
        |result| format!("{} (exit {})", result.label(), result.exit_code),
    )
}

fn format_command(command: &CommandInvocation) -> String {
    std::iter::once(&command.program)
        .chain(command.arguments.iter())
        .map(|argument| format!("`{}`", markdown_cell(&argument.to_string_lossy())))
        .collect::<Vec<_>>()
        .join(" ")
}

fn markdown_cell(value: &str) -> String {
    value
        .replace(['\n', '\r'], " ")
        .replace('|', "\\|")
        .replace('`', "\\`")
}

struct Invocation {
    report_path: Option<PathBuf>,
    command: Option<CommandInvocation>,
}

struct CommandInvocation {
    program: OsString,
    arguments: Vec<OsString>,
}

impl Invocation {
    fn parse(mut arguments: Vec<OsString>) -> Result<Self, String> {
        let mut report_path = None;
        while let Some(argument) = arguments.first() {
            if argument == "--help" || argument == "-h" {
                print_help();
                process::exit(0);
            }
            if argument == "--report" {
                arguments.remove(0);
                let path = arguments
                    .first()
                    .cloned()
                    .ok_or_else(|| "expected a report path after `--report`".to_owned())?;
                arguments.remove(0);
                let path = PathBuf::from(path);
                if report_path.replace(path).is_some() {
                    return Err("`--report` may only be supplied once".to_owned());
                }
                continue;
            }
            if argument == "--" {
                arguments.remove(0);
            }
            break;
        }
        let mut arguments = arguments.into_iter();
        let command = arguments.next().map(|program| CommandInvocation {
            program,
            arguments: arguments.collect(),
        });
        if command.is_none() && report_path.is_none() {
            return Err(
                "expected a command after `--`, or `--report <path>` for a host capability report"
                    .to_owned(),
            );
        }
        Ok(Self {
            report_path,
            command,
        })
    }
}

fn print_help() {
    println!(
        "Usage: rust-browser-proofs [--report <path>] [--] <command> [args...]\n\
         \n\
         Runs the command with rustup's selected rustc and cargo first on PATH.\n\
         \n\
         `--report <path>` writes the host and invocation status as Markdown.\n\
         Run wasm-pack from a Cargo package directory.\n\
         Example: rust-browser-proofs -- wasm-pack test --headless --chrome"
    );
}

#[cfg(test)]
mod tests {
    use super::{
        CommandInvocation, CommandResult, Invocation, browser_test_status,
        virtual_workspace_wasm_pack_error,
    };
    use std::ffi::{OsStr, OsString};
    use std::path::PathBuf;

    #[test]
    fn parses_a_command_after_the_separator() {
        let invocation = Invocation::parse(vec![
            OsString::from("--"),
            OsString::from("wasm-pack"),
            OsString::from("test"),
        ])
        .unwrap();

        let command = invocation.command.unwrap();
        assert_eq!(command.program, "wasm-pack");
        assert_eq!(command.arguments, [OsString::from("test")]);
    }

    #[test]
    fn rejects_a_missing_command() {
        assert!(Invocation::parse(vec![OsString::from("--")]).is_err());
    }

    #[test]
    fn parses_a_report_only_invocation() {
        let invocation = Invocation::parse(vec![
            OsString::from("--report"),
            OsString::from("browser-status.md"),
        ])
        .unwrap();

        assert_eq!(
            invocation.report_path,
            Some(PathBuf::from("browser-status.md"))
        );
        assert!(invocation.command.is_none());
    }

    #[test]
    fn rejects_a_report_flag_without_a_path() {
        let error = match Invocation::parse(vec![OsString::from("--report")]) {
            Ok(_) => panic!("an output path is required after --report"),
            Err(error) => error,
        };

        assert!(error.contains("report path"));
    }

    #[test]
    fn reports_only_the_explicit_browser_as_executed() {
        let invocation = Invocation {
            report_path: None,
            command: Some(CommandInvocation {
                program: OsString::from("wasm-pack"),
                arguments: vec![OsString::from("test"), OsString::from("--chrome")],
            }),
        };
        let result = CommandResult { exit_code: 0 };

        assert_eq!(
            browser_test_status(&invocation, Some(&result), "--chrome"),
            "passed (exit 0)"
        );
        assert_eq!(
            browser_test_status(&invocation, Some(&result), "--firefox"),
            "not identified by this command"
        );
    }

    #[test]
    fn rejects_wasm_pack_from_a_virtual_workspace() {
        let error = virtual_workspace_wasm_pack_error(
            OsStr::new("wasm-pack"),
            &[],
            "[workspace]\nmembers = [\"fixture\"]\n",
        )
        .unwrap();

        assert!(error.contains("Cargo package directory"));
    }
}
