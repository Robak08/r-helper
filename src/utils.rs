// Utility functions shared across the app

use anyhow::Result;
use std::process::Command;

// System Command Execution

/// Execute a PowerShell script with consistent configuration
///
/// This function provides a centralized way to execute PowerShell commands
/// with proper error handling and consistent flags.
#[cfg(target_os = "windows")]
pub fn execute_powershell_command(script: &str) -> Result<String> {
    use std::os::windows::process::CommandExt;

    let mut cmd = Command::new(POWERSHELL_PATH);
    cmd.args(&["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"])
        .arg(script)
        .creation_flags(CREATE_NO_WINDOW);

    match cmd.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr_string = String::from_utf8_lossy(&output.stderr);
            let stderr = stderr_string.trim();

            if !stderr.is_empty() && output.status.code() != Some(0) {
                Err(anyhow::anyhow!("PowerShell error: {}", stderr))
            } else {
                Ok(stdout)
            }
        }
        Err(e) => Err(anyhow::anyhow!("Failed to execute PowerShell: {}", e)),
    }
}

#[cfg(not(target_os = "windows"))]
pub fn execute_powershell_command(_script: &str) -> Result<String> {
    Err(anyhow::anyhow!("PowerShell is only available on Windows"))
}

// String Processing Utilities

/// Clean and format strings for display
pub fn clean_display_string(input: &str) -> String {
    input
        .trim()
        .replace('\r', "")
        .replace('\n', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// Constants

/// PowerShell executable path on Windows
#[cfg(target_os = "windows")]
pub const POWERSHELL_PATH: &str = "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe";

/// Windows creation flag to hide console window
#[cfg(target_os = "windows")]
pub const CREATE_NO_WINDOW: u32 = 0x08000000;
