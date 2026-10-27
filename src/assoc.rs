//! Windows file associations: registers IINA-RS under HKEY_CURRENT_USER so it
//! appears in "Open with" for media files. Windows 10/11 don't let apps set
//! themselves as the default, so the user then confirms it in Settings.
//! Only runs when the user presses the button in Preferences.

#[cfg(windows)]
mod imp {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    const PROG_ID: &str = "IINA-RS.Media";

    fn exe() -> Result<String, String> {
        std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).map_err(|e| e.to_string())
    }

    fn notify_shell() {
        use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED as i32, SHCNF_IDLIST, std::ptr::null(), std::ptr::null()) };
    }

    pub fn register(exts: &[&str]) -> Result<(), String> {
        let exe = exe()?;
        let classes = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags("Software\\Classes", winreg::enums::KEY_ALL_ACCESS).map_err(|e| e.to_string())?;
        let (prog, _) = classes.create_subkey(PROG_ID).map_err(|e| e.to_string())?;
        prog.set_value("", &"Media file (IINA-RS)").map_err(|e| e.to_string())?;
        let (icon, _) = prog.create_subkey("DefaultIcon").map_err(|e| e.to_string())?;
        icon.set_value("", &format!("\"{exe}\",0")).map_err(|e| e.to_string())?;
        let (cmd, _) = prog.create_subkey("shell\\open\\command").map_err(|e| e.to_string())?;
        cmd.set_value("", &format!("\"{exe}\" \"%1\"")).map_err(|e| e.to_string())?;

        let (app, _) = classes.create_subkey("Applications\\iina-rs.exe").map_err(|e| e.to_string())?;
        app.set_value("FriendlyAppName", &"IINA-RS").map_err(|e| e.to_string())?;
        let (app_cmd, _) = app.create_subkey("shell\\open\\command").map_err(|e| e.to_string())?;
        app_cmd.set_value("", &format!("\"{exe}\" \"%1\"")).map_err(|e| e.to_string())?;
        let (types, _) = app.create_subkey("SupportedTypes").map_err(|e| e.to_string())?;

        for ext in exts {
            let (k, _) = classes.create_subkey(format!(".{ext}\\OpenWithProgids")).map_err(|e| e.to_string())?;
            k.set_value(PROG_ID, &"").map_err(|e| e.to_string())?;
            types.set_value(format!(".{ext}"), &"").map_err(|e| e.to_string())?;
        }
        notify_shell();
        Ok(())
    }

    /// True if "Open with" currently points at this executable (so an
    /// uninstaller doesn't remove a registration made by another copy).
    pub fn registered_to_this_exe() -> bool {
        let Ok(exe) = exe() else { return false };
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey("Software\\Classes\\Applications\\iina-rs.exe\\shell\\open\\command")
            .and_then(|k| k.get_value::<String, _>(""))
            .is_ok_and(|cmd| cmd.to_lowercase().contains(&exe.to_lowercase()))
    }

    pub fn unregister(exts: &[&str]) -> Result<(), String> {
        let classes = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags("Software\\Classes", winreg::enums::KEY_ALL_ACCESS).map_err(|e| e.to_string())?;
        let _ = classes.delete_subkey_all(PROG_ID);
        let _ = classes.delete_subkey_all("Applications\\iina-rs.exe");
        for ext in exts {
            if let Ok(k) = classes.open_subkey_with_flags(format!(".{ext}\\OpenWithProgids"), winreg::enums::KEY_ALL_ACCESS) {
                let _ = k.delete_value(PROG_ID);
            }
        }
        notify_shell();
        Ok(())
    }

    pub fn open_default_apps_settings() {
        let _ = std::process::Command::new("cmd").args(["/C", "start", "", "ms-settings:defaultapps"]).spawn();
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn register(_: &[&str]) -> Result<(), String> {
        Err("Only needed on Windows; on Linux the .desktop file declares the media types.".into())
    }
    pub fn unregister(_: &[&str]) -> Result<(), String> {
        Ok(())
    }
    pub fn registered_to_this_exe() -> bool {
        false
    }
    pub fn open_default_apps_settings() {}
}

pub use imp::*;
