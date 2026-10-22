//! The `Quickshell` singleton: shell paths, environment, detached processes and version checks.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element = "QuickshellCore"]
        #[qml_singleton]
        #[qproperty(QString, shell_dir, cxx_name = "shellDir", READ = shell_dir, CONSTANT)]
        #[qproperty(QString, config_dir, cxx_name = "configDir", READ = shell_dir, CONSTANT)]
        #[qproperty(QString, shell_root, cxx_name = "shellRoot", READ = shell_dir, CONSTANT)]
        #[qproperty(QString, data_dir, cxx_name = "dataDir", READ = data_dir, CONSTANT)]
        #[qproperty(QString, state_dir, cxx_name = "stateDir", READ = state_dir, CONSTANT)]
        #[qproperty(QString, cache_dir, cxx_name = "cacheDir", READ = cache_dir, CONSTANT)]
        #[qproperty(QString, shell_id, cxx_name = "shellId", READ = shell_id, CONSTANT)]
        #[qproperty(i32, process_id, cxx_name = "processId", READ = process_id, CONSTANT)]
        #[qproperty(QString, working_directory, cxx_name = "workingDirectory", READ = working_directory, WRITE = set_working_directory, NOTIFY)]
        #[qproperty(bool, watch_files, cxx_name = "watchFiles", READ = watch_files, WRITE = set_watch_files, NOTIFY)]
        type QuickshellGlobal = super::QuickshellGlobalRust;

        fn shell_dir(self: &QuickshellGlobal) -> QString;
        fn data_dir(self: &QuickshellGlobal) -> QString;
        fn state_dir(self: &QuickshellGlobal) -> QString;
        fn cache_dir(self: &QuickshellGlobal) -> QString;
        fn shell_id(self: &QuickshellGlobal) -> QString;
        fn process_id(self: &QuickshellGlobal) -> i32;
        fn working_directory(self: &QuickshellGlobal) -> QString;
        fn set_working_directory(self: Pin<&mut QuickshellGlobal>, dir: QString);
        fn watch_files(self: &QuickshellGlobal) -> bool;
        fn set_watch_files(self: Pin<&mut QuickshellGlobal>, on: bool);

        #[qinvokable]
        fn env(self: &QuickshellGlobal, name: &QString) -> QVariant;
        #[qinvokable]
        #[cxx_name = "execDetached"]
        fn exec_detached(self: &QuickshellGlobal, context: &QVariant);
        #[qinvokable]
        #[cxx_name = "shellPath"]
        fn shell_path(self: &QuickshellGlobal, path: &QString) -> QString;
        #[qinvokable]
        #[cxx_name = "configPath"]
        fn config_path(self: &QuickshellGlobal, path: &QString) -> QString;
        #[qinvokable]
        #[cxx_name = "dataPath"]
        fn data_path(self: &QuickshellGlobal, path: &QString) -> QString;
        #[qinvokable]
        #[cxx_name = "statePath"]
        fn state_path(self: &QuickshellGlobal, path: &QString) -> QString;
        #[qinvokable]
        #[cxx_name = "cachePath"]
        fn cache_path(self: &QuickshellGlobal, path: &QString) -> QString;
        #[qinvokable]
        #[cxx_name = "iconPath"]
        fn icon_path(self: &QuickshellGlobal, icon: &QString) -> QString;
        #[qinvokable]
        #[cxx_name = "iconPath"]
        fn icon_path_fallback(self: &QuickshellGlobal, icon: &QString, fallback: &QVariant) -> QString;
        #[qinvokable]
        #[cxx_name = "hasVersion"]
        fn has_version(self: &QuickshellGlobal, major: i32, minor: i32) -> bool;
        #[qinvokable]
        #[cxx_name = "hasVersion"]
        fn has_version_features(self: &QuickshellGlobal, major: i32, minor: i32, features: &QStringList) -> bool;
        #[qinvokable]
        #[cxx_name = "hasThemeIcon"]
        fn has_theme_icon(self: &QuickshellGlobal, icon: &QString) -> bool;
        #[qinvokable]
        #[cxx_name = "hasQtVersion"]
        fn has_qt_version(self: &QuickshellGlobal, major: i32, minor: i32) -> bool;
        #[qinvokable]
        fn reload(self: &QuickshellGlobal, hard: bool);
        #[qinvokable]
        #[cxx_name = "inhibitReloadPopup"]
        fn inhibit_reload_popup(self: &QuickshellGlobal);
    }
}

use crate::common::shell;
use core::pin::Pin;
use cxx_qt_lib::{QString, QStringList, QVariant};
use std::path::Path;

/// The Quickshell release whose QML API this host implements.
pub const COMPAT_VERSION: (i32, i32) = (0, 3);
/// Upstream's "unreleased features" list, here what qs-rs runs that Quickshell does not:
/// `Quickshell.hasVersion(0, 3, ["webengine"])` holds only on qs-rs. Quickshell hands Qt no argv,
/// and Chromium needs argv[0] (inir's patched build sets it); qs-rs passes one.
/// "screencopy-luminance": ScreencopyView.meanLuminance(), the frame's mean Rec. 709 luma.
/// "toplevel-capture": ScreencopyView records a Toplevel on niri, through its ScreenCast D-Bus API;
/// Quickshell has no window capture there. "niri-window-capture": ScreencopyView.niriWindowId, a
/// qs-rs property (a file that sets it fails to load anywhere else, so it gets its own name).
/// "screencopy-source-rect": ScreencopyView.sourceRect, a live crop that follows window resizes.
pub const FEATURES: &[&str] = &["webengine", "screencopy-luminance", "toplevel-capture", "niri-window-capture", "native-spring", "screencopy-source-rect"];

#[derive(Default)]
pub struct QuickshellGlobalRust {}

fn qs(p: &Path) -> QString {
    QString::from(&*p.to_string_lossy())
}

fn join(dir: &Path, path: &QString) -> QString {
    QString::from(&format!("{}/{}", dir.to_string_lossy(), path))
}

impl qobject::QuickshellGlobal {
    fn shell_dir(&self) -> QString {
        qs(&shell().shell_dir)
    }
    fn data_dir(&self) -> QString {
        qs(&shell().data_dir)
    }
    fn state_dir(&self) -> QString {
        qs(&shell().state_dir)
    }
    fn cache_dir(&self) -> QString {
        qs(&shell().cache_dir)
    }
    fn shell_id(&self) -> QString {
        QString::from(&shell().shell_id)
    }
    fn process_id(&self) -> i32 {
        std::process::id() as i32
    }

    fn working_directory(&self) -> QString {
        std::env::current_dir().map(|d| qs(&d)).unwrap_or_default()
    }

    fn watch_files(&self) -> bool {
        crate::common::watch_files()
    }

    fn set_watch_files(self: Pin<&mut Self>, on: bool) {
        if crate::common::set_watch_files(on) {
            self.watch_files_changed();
        }
    }

    fn set_working_directory(self: Pin<&mut Self>, dir: QString) {
        if std::env::set_current_dir(dir.to_string()).is_ok() {
            self.working_directory_changed();
        }
    }

    fn env(&self, name: &QString) -> QVariant {
        match std::env::var(name.to_string()) {
            Ok(v) => QVariant::from(&QString::from(&v)),
            Err(_) => crate::util::null_variant(),
        }
    }

    fn exec_detached(&self, context: &QVariant) {
        let args = crate::proc::context_from_variant(context);
        let ctx = crate::proc::Context {
            command: args.command.unwrap_or_default(),
            working_directory: args.working_directory.unwrap_or_default(),
            environment: args.environment.as_ref().map(crate::proc::env_from_hash).unwrap_or_default(),
            clear_environment: args.clear_environment.unwrap_or(false),
        };
        if ctx.command.is_empty() {
            crate::qs_log!(warn, "quickshell", "Cannot start process as command is empty.");
            return;
        }
        crate::proc::spawn_detached(&ctx);
    }

    fn shell_path(&self, path: &QString) -> QString {
        join(&shell().shell_dir, path)
    }
    fn config_path(&self, path: &QString) -> QString {
        self.shell_path(path)
    }
    fn data_path(&self, path: &QString) -> QString {
        join(&shell().data_dir, path)
    }
    fn state_path(&self, path: &QString) -> QString {
        join(&shell().state_dir, path)
    }
    fn cache_path(&self, path: &QString) -> QString {
        join(&shell().cache_dir, path)
    }

    fn icon_path(&self, icon: &QString) -> QString {
        crate::icons::request_string(&icon.to_string(), None)
    }

    /// `iconPath(icon, check: bool)` returns "" for a missing icon; `iconPath(icon, fallback)`
    /// falls back to another icon name.
    fn icon_path_fallback(&self, icon: &QString, fallback: &QVariant) -> QString {
        let icon = icon.to_string();
        if let Some(check) = fallback.value::<bool>().filter(|_| fallback.type_name() == "bool") {
            if check && crate::icons::lookup(&icon).is_none() {
                return QString::default();
            }
            return crate::icons::request_string(&icon, None);
        }
        let fb = fallback.value::<QString>().map(|s| s.to_string());
        crate::icons::request_string(&icon, fb.as_deref())
    }

    fn has_version(&self, major: i32, minor: i32) -> bool {
        (major, minor) <= COMPAT_VERSION
    }

    fn has_version_features(&self, major: i32, minor: i32, features: &QStringList) -> bool {
        // Same order as upstream's PreprocEnv::hasVersion: an older version passes whatever
        // the features, the current one only with all of them.
        if (major, minor) < COMPAT_VERSION {
            return true;
        }
        let all = (0..features.len()).all(|i| features.get(i).is_some_and(|f| FEATURES.contains(&f.to_string().as_str())));
        all && (major, minor) == COMPAT_VERSION
    }

    fn inhibit_reload_popup(&self) {}

    fn has_theme_icon(&self, icon: &QString) -> bool {
        crate::util::has_theme_icon(icon)
    }

    fn has_qt_version(&self, major: i32, minor: i32) -> bool {
        let v = crate::util::qt_version().to_string();
        let mut parts = v.split('.').map(|p| p.parse::<i32>().unwrap_or(0));
        (parts.next().unwrap_or(0), parts.next().unwrap_or(0)) >= (major, minor)
    }

    fn reload(&self, hard: bool) {
        crate::common::request_reload(hard);
    }
}
