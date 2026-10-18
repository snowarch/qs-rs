//! Launch-time configuration read from `//@ pragma` lines at the top of the root shell file,
//! applied before Qt starts (same set and rules as Quickshell's launch.cpp).

#[derive(Debug, Default, PartialEq)]
pub struct Pragmas {
    pub use_qapplication: bool,
    pub native_text_rendering: bool,
    pub desktop_settings_aware: bool,
    pub use_system_style: bool,
    pub drop_expensive_fonts: bool,
    pub icon_theme: Option<String>,
    pub app_id: Option<String>,
    pub shell_id: Option<String>,
    pub data_dir: Option<String>,
    pub state_dir: Option<String>,
    pub cache_dir: Option<String>,
    pub env: Vec<(String, String)>,
    pub default_env: Vec<(String, String)>,
}

pub fn parse(text: &str) -> Result<Pragmas, String> {
    let mut p = Pragmas { desktop_settings_aware: true, ..Default::default() };
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("import") {
            break;
        }
        let Some(pragma) = line.strip_prefix("//@ pragma ") else { continue };
        let pragma = pragma.trim();
        let arg = |name: &str| pragma.strip_prefix(name).map(|v| v.trim().to_string());
        if let Some(kv) = arg("Env ").or_else(|| arg("DefaultEnv ")) {
            let (k, v) = kv.split_once('=').ok_or_else(|| format!("Env pragma {pragma:?} not in the form 'VAR = VALUE'"))?;
            let entry = (k.trim().to_string(), v.trim().to_string());
            if pragma.starts_with("Env ") { p.env.push(entry) } else { p.default_env.push(entry) }
        } else if pragma == "UseQApplication" {
            p.use_qapplication = true;
        } else if pragma == "NativeTextRendering" {
            p.native_text_rendering = true;
        } else if pragma == "IgnoreSystemSettings" {
            p.desktop_settings_aware = false;
        } else if pragma == "RespectSystemStyle" {
            p.use_system_style = true;
        } else if pragma == "DropExpensiveFonts" {
            p.drop_expensive_fonts = true;
        } else if let Some(v) = arg("IconTheme ") {
            p.icon_theme = Some(v);
        } else if let Some(v) = arg("AppId ") {
            p.app_id = Some(v);
        } else if let Some(v) = arg("ShellId ") {
            p.shell_id = Some(v);
        } else if let Some(v) = arg("DataDir ") {
            p.data_dir = Some(v);
        } else if let Some(v) = arg("StateDir ") {
            p.state_dir = Some(v);
        } else if let Some(v) = arg("CacheDir ") {
            p.cache_dir = Some(v);
        } else {
            eprintln!("Unrecognized pragma {pragma:?}");
        }
    }
    Ok(p)
}

/// Applies the environment side of the pragmas. Must run before any thread or Qt object exists.
pub fn apply_env(p: &Pragmas) {
    // SAFETY: called from main before other threads are spawned.
    unsafe {
        if !p.use_system_style {
            std::env::remove_var("QT_STYLE_OVERRIDE");
            std::env::set_var("QT_QUICK_CONTROLS_STYLE", "Fusion");
        }
        for (k, v) in &p.default_env {
            if std::env::var_os(k).is_none() {
                std::env::set_var(k, v);
            }
        }
        for (k, v) in &p.env {
            std::env::set_var(k, v);
        }
        // Every window shares one GL context (quickshell's window.h), which needs the basic render
        // loop whatever the shell asks for; Mesa's per-context driver thread is then only
        // overhead. QSRS_PER_WINDOW_GL=1 keeps upstream's setup.
        if std::env::var_os("QSRS_PER_WINDOW_GL").is_none() {
            std::env::set_var("QSG_RENDER_LOOP", "basic");
            if std::env::var_os("GALLIUM_THREAD").is_none() {
                std::env::set_var("GALLIUM_THREAD", "0");
            }
        }
        // radeonsi recompiles an optimized variant of each shader on its own threads at every
        // start, uncached: 1.7 s of CPU and a ~200 MB heap spike opening inir's control panel
        // (measured A/B; Quickshell does the same). The unoptimized one it would draw
        // with meanwhile is what we keep. Children get the environment captured before this.
        if std::env::var_os("AMD_DEBUG").is_none() {
            std::env::set_var("AMD_DEBUG", "nooptvariant");
        }
        // Measurement override: QSRS_ENV_OVERRIDE="K=V K2=V2" wins over the shell's pragmas.
        if let Ok(o) = std::env::var("QSRS_ENV_OVERRIDE") {
            for kv in o.split_whitespace() {
                if let Some((k, v)) = kv.split_once('=') {
                    std::env::set_var(k, v);
                }
            }
        }
    }
}

/// Upstream's DropExpensiveFonts (also `QS_DROP_EXPENSIVE_FONTS=1`): a fontconfig file in the
/// instance run dir that includes the user's config and rejects woff/woff2 wrapped fonts.
pub fn drop_expensive_fonts(p: &Pragmas, run_dir: &std::path::Path) {
    if !p.drop_expensive_fonts && std::env::var("QS_DROP_EXPENSIVE_FONTS").as_deref() != Ok("1") {
        return;
    }
    let base = std::env::var("FONTCONFIG_FILE").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| "/etc/fonts/fonts.conf".into());
    let reject = |w: &str| format!("\t\t\t<pattern>\n\t\t\t\t<patelt name=\"fontwrapper\">\n\t\t\t\t\t<string>{w}</string>\n\t\t\t\t</patelt>\n\t\t\t</pattern>\n");
    let xml = format!(
        "<?xml version=\"1.0\"?>\n<!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\">\n<fontconfig>\n\t<include ignore_missing=\"no\">{base}</include>\n\t<selectfont>\n\t\t<rejectfont>\n{}{}\t\t</rejectfont>\n\t</selectfont>\n</fontconfig>\n",
        reject("woff"),
        reject("woff2")
    );
    let path = run_dir.join("fonts-override.conf");
    match std::fs::write(&path, xml) {
        // SAFETY: called from main before other threads are spawned.
        Ok(()) => unsafe { std::env::set_var("FONTCONFIG_FILE", &path) },
        Err(e) => eprintln!("Could not write fontconfig filter to {}: {e}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_inir_header() {
        let p = parse(
            "//@ pragma UseQApplication\n//@ pragma ShellId inir\n//-@ pragma EnableQtWebEngineQuick\n\
             //@ pragma Env QS_NO_RELOAD_POPUP=1\n//@ pragma DefaultEnv QT_LOGGING_RULES=a=false;b=false\n\
             import QtQuick\n//@ pragma ShellId ignored\n",
        )
        .unwrap();
        assert!(p.use_qapplication);
        assert_eq!(p.shell_id.as_deref(), Some("inir"));
        assert_eq!(p.env, vec![("QS_NO_RELOAD_POPUP".into(), "1".into())]);
        assert_eq!(p.default_env, vec![("QT_LOGGING_RULES".into(), "a=false;b=false".into())]);
    }

    #[test]
    fn rejects_env_without_equals() {
        assert!(parse("//@ pragma Env NOPE\n").is_err());
    }
}
