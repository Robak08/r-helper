#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

#[cfg(windows)]
const APP_REGISTRY_NAME: &str = "R-Helper";

#[cfg(windows)]
pub fn is_startup_enabled() -> bool {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    hkcu.open_subkey(RUN_KEY)
        .and_then(|run| run.get_value::<String, _>(APP_REGISTRY_NAME))
        .map(|path| path == current_exe_path() || path == startup_command())
        .unwrap_or(false)
}

#[cfg(not(windows))]
pub fn is_startup_enabled() -> bool {
    false
}

#[cfg(windows)]
pub fn set_startup_enabled(enabled: bool) -> anyhow::Result<()> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run, _) = hkcu.create_subkey(RUN_KEY)?;

    if enabled {
        run.set_value(APP_REGISTRY_NAME, &startup_command())?;
    } else {
        let _ = run.delete_value(APP_REGISTRY_NAME);
    }

    Ok(())
}

#[cfg(not(windows))]
pub fn set_startup_enabled(_enabled: bool) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(windows)]
fn current_exe_path() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "rhelper.exe".to_string())
}

#[cfg(windows)]
fn startup_command() -> String {
    format!("\"{}\" --startup", current_exe_path())
}
