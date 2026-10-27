//! Shell scanner: walks the QML files reachable from the root file, synthesizes a qmldir per
//! directory (so `import qs.foo` and same-dir singletons work) and mirrors the shell into a VFS
//! of symlinks that the QML engine loads from. Same rules as Quickshell's scan.cpp.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Default, Debug)]
pub struct Scan {
    pub root: PathBuf,
    /// Directories that got scanned, in scan order.
    pub dirs: Vec<PathBuf>,
    /// QML/JS files read during the scan.
    pub files: BTreeSet<PathBuf>,
    /// Synthesized file contents keyed by real path (qmldir files).
    pub intercepts: BTreeMap<PathBuf, String>,
    /// Content hash of each file read, what `Quickshell.watchFiles` compares a change against.
    pub hashes: BTreeMap<PathBuf, u64>,
}

pub fn content_hash(data: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    data.hash(&mut h);
    h.finish()
}

#[derive(Default, Debug)]
struct FileInfo {
    singleton: bool,
    internal: bool,
}

impl Scan {
    pub fn new(root_file: &Path) -> io::Result<Scan> {
        let root_file = root_file.canonicalize()?;
        let mut scan = Scan { root: root_file.parent().unwrap().to_path_buf(), ..Default::default() };
        scan.scan_file(&root_file);
        // Files reached only by URL (Loader.source, Qt.createComponent, inir's getWidgetUrl) are
        // not in the import graph, yet their `root:` imports need the same rewrite: scan every dir
        // of the shell that holds QML. Hidden dirs are skipped, and links are followed only under
        // modules/, where `inir hub` links plugins (a link elsewhere, like inir's dev/, is not the shell).
        let modules = scan.root.join("modules");
        let mut seen = BTreeSet::new();
        let mut stack = vec![scan.root.clone()];
        while let Some(dir) = stack.pop() {
            if !dir.canonicalize().is_ok_and(|real| seen.insert(real)) {
                continue;
            }
            let Ok(rd) = fs::read_dir(&dir) else { continue };
            let mut has_qml = false;
            let mut lower = Vec::new();
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let Ok(ft) = e.file_type() else { continue };
                let linked_dir = ft.is_symlink() && e.path().starts_with(&modules) && e.path().is_dir();
                if (ft.is_dir() || linked_dir) && !name.starts_with('.') {
                    stack.push(e.path());
                } else if ft.is_file() && name.ends_with(".qml") {
                    has_qml = true;
                    if !name.starts_with(|c: char| c.is_uppercase()) {
                        lower.push(e.path());
                    }
                }
            }
            if has_qml {
                scan.scan_dir(&dir);
                for f in lower {
                    scan.scan_file(&f);
                }
            }
        }
        Ok(scan)
    }

    fn scan_dir(&mut self, dir: &Path) {
        if self.dirs.iter().any(|d| d == dir) {
            return;
        }
        self.dirs.push(dir.to_path_buf());
        let Ok(rd) = fs::read_dir(dir) else { return };
        let mut names: Vec<String> =
            rd.flatten().filter(|e| e.path().is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        let mut entries = Vec::new();
        for name in &names {
            if name.starts_with(|c: char| c.is_uppercase()) && name.ends_with(".qml")
                && let Some(info) = self.scan_file(&dir.join(name)) {
                    entries.push((name.clone(), info));
                }
        }
        // A shell-provided qmldir wins; its files are still scanned for imports.
        if names.iter().any(|n| n == "qmldir") {
            return;
        }
        let mut qmldir = String::new();
        if let Ok(rel) = dir.strip_prefix(&self.root) {
            let rel = rel.to_string_lossy();
            if rel.chars().all(|c| c == '/' || c.is_ascii_alphanumeric() || c == '_') {
                let module = if rel.is_empty() { String::new() } else { format!(".{}", rel.replace('/', ".")) };
                qmldir.push_str(&format!("module qs{module}\n"));
            }
        }
        for (name, info) in entries {
            if info.internal {
                qmldir.push_str("internal ");
            }
            if info.singleton {
                qmldir.push_str("singleton ");
            }
            qmldir.push_str(&format!("{} 1.0 {}\n", &name[..name.len() - 4], name));
        }
        self.intercepts.insert(dir.join("qmldir"), qmldir);
    }

    fn scan_file(&mut self, path: &Path) -> Option<FileInfo> {
        if !self.files.insert(path.to_path_buf()) {
            return None;
        }
        let text = fs::read_to_string(path).ok()?;
        self.hashes.insert(path.to_path_buf(), content_hash(text.as_bytes()));
        let mut info = FileInfo::default();
        let mut imports = Vec::new();
        // `import "root:X"` lines, rewritten to relative paths in the VFS copy (Qt treats an
        // unknown scheme in a directory import as remote and cannot read its qmldir).
        let mut rewritten = false;
        let mut header = Vec::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line == "pragma Singleton" {
                info.singleton = true;
            } else if line == "//@ pragma Internal" {
                info.internal = true;
            } else if line.starts_with("import") {
                if let Some(at) = line.find(" qs.") {
                    let module: String = line[at + 4..].chars().take_while(|c| *c != ' ').collect();
                    if module.chars().all(|c| c == '.' || c.is_ascii_alphanumeric() || c == '_') {
                        imports.push(self.root.join(module.replace('.', "/")));
                    }
                } else if let (Some(a), Some(b)) = (line.find('"'), line.rfind('"'))
                    && b > a + 1 {
                        let target = &line[a + 1..b];
                        let base = path.parent().unwrap();
                        imports.push(match target.strip_prefix("root:") {
                            Some(r) => {
                                let r = r.trim_start_matches('/');
                                let depth = base.strip_prefix(&self.root).map(|p| p.components().count()).unwrap_or(0);
                                let rel = format!("{}{r}", "../".repeat(depth));
                                let rel = if rel.is_empty() { ".".to_string() } else { rel };
                                header.push(raw.replacen(&format!("\"{target}\""), &format!("\"{rel}\""), 1));
                                rewritten = true;
                                self.root.join(r)
                            }
                            None => {
                                header.push(raw.to_string());
                                base.join(target)
                            }
                        });
                        continue;
                    }
            } else if line.contains('{') {
                break;
            }
            header.push(raw.to_string());
        }
        if rewritten {
            let rest: Vec<&str> = text.lines().skip(header.len()).collect();
            let mut out = header.join("\n");
            out.push('\n');
            out.push_str(&rest.join("\n"));
            out.push('\n');
            self.intercepts.insert(path.to_path_buf(), out);
        }
        self.scan_dir(path.parent().unwrap());
        for import in imports {
            if let Some(dir) = self.resolve(&import) {
                self.scan_dir(&dir);
            }
        }
        Some(info)
    }

    /// An imported dir as Quickshell sees it: its path in the shell when it is there, through links
    /// (QFileInfo::absoluteFilePath, no link resolved), else the real one.
    fn resolve(&self, import: &Path) -> Option<PathBuf> {
        let mut lexical = PathBuf::new();
        for c in import.components() {
            match c {
                std::path::Component::ParentDir => drop(lexical.pop()),
                std::path::Component::CurDir => {}
                c => lexical.push(c),
            }
        }
        if lexical.starts_with(&self.root) && lexical.is_dir() {
            return Some(lexical);
        }
        import.canonicalize().ok().filter(|d| d.is_dir())
    }

    /// Mirror the shell into `vfs`: real dirs for scanned dirs, symlinks for everything else,
    /// synthesized files written in place. Entries that are already right stay untouched (same
    /// path and mtime keep Qt's QML disk cache valid across starts); stale ones are removed.
    pub fn write_vfs(&self, vfs: &Path) -> io::Result<()> {
        // Every ancestor of a scanned dir inside the root must be a real dir too.
        let mut real: BTreeSet<PathBuf> = BTreeSet::new();
        for d in &self.dirs {
            if let Ok(rel) = d.strip_prefix(&self.root) {
                let mut p = self.root.clone();
                real.insert(p.clone());
                for c in rel.components() {
                    p.push(c);
                    real.insert(p.clone());
                }
            }
        }
        let mut keep: BTreeSet<PathBuf> = BTreeSet::new();
        for dir in &real {
            let out = vfs.join(dir.strip_prefix(&self.root).unwrap());
            if !fs::symlink_metadata(&out).is_ok_and(|m| m.is_dir()) {
                remove_any(&out);
                fs::create_dir_all(&out)?;
            }
            keep.insert(out.clone());
            for entry in fs::read_dir(dir)?.flatten() {
                let src = entry.path();
                if real.contains(&src) || self.intercepts.contains_key(&src) {
                    continue;
                }
                let dst = out.join(entry.file_name());
                if fs::read_link(&dst).is_ok_and(|t| t == src) {
                    keep.insert(dst);
                    continue;
                }
                remove_any(&dst);
                std::os::unix::fs::symlink(&src, &dst)?;
                keep.insert(dst);
            }
        }
        for (path, content) in &self.intercepts {
            if let Ok(rel) = path.strip_prefix(&self.root) {
                let dst = vfs.join(rel);
                let same = fs::symlink_metadata(&dst).is_ok_and(|m| m.is_file())
                    && fs::read(&dst).is_ok_and(|c| c == content.as_bytes());
                if !same {
                    remove_any(&dst);
                    fs::write(&dst, content)?;
                }
                keep.insert(dst);
            }
        }
        prune(vfs, &keep)
    }
}

fn remove_any(p: &Path) {
    match fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => drop(fs::remove_dir_all(p)),
        Ok(_) => drop(fs::remove_file(p)),
        Err(_) => {}
    }
}

/// Removes everything under `dir` that is not in `keep`.
fn prune(dir: &Path, keep: &BTreeSet<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)?.flatten() {
        let p = entry.path();
        if !keep.contains(&p) {
            remove_any(&p);
        } else if entry.file_type()?.is_dir() {
            prune(&p, keep)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("qsrs-scan-{}-{}", std::process::id(), files.len()));
        let _ = fs::remove_dir_all(&dir);
        for (p, c) in files {
            let p = dir.join(p);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, c).unwrap();
        }
        dir
    }

    #[test]
    fn rewrites_root_imports_of_files_loaded_by_url() {
        // inir's ShellIiPanels.qml loads modules/ii/ShellIiPanelsImpl.qml by Loader.source, and
        // only that file imports modules/shared/tilingOverlay, whose `import "root:"` Qt reads as remote.
        let dir = tree(&[
            ("shell.qml", "import QtQuick\nLoader { source: \"panels/Impl.qml\" }\n"),
            ("panels/Impl.qml", "import qs.overlay\nItem { Overlay {} }\n"),
            ("overlay/Overlay.qml", "import QtQuick\nimport \"root:\"\nimport \"root:services\"\nItem {}\n"),
            ("tools/picker.qml", "import \"root:services\"\nItem {}\n"),
            (".git/Hidden.qml", "import \"root:\"\nItem {}\n"),
        ]);
        let scan = Scan::new(&dir.join("shell.qml")).unwrap();
        let root = dir.canonicalize().unwrap();
        assert_eq!(scan.intercepts[&root.join("overlay/Overlay.qml")], "import QtQuick\nimport \"../\"\nimport \"../services\"\nItem {}\n");
        assert_eq!(scan.intercepts[&root.join("tools/picker.qml")], "import \"../services\"\nItem {}\n");
        assert_eq!(scan.intercepts[&root.join("overlay/qmldir")], "module qs.overlay\nOverlay 1.0 Overlay.qml\n");
        assert!(!scan.intercepts.contains_key(&root.join(".git/Hidden.qml")));
    }

    #[test]
    fn synthesizes_qmldir_for_imported_modules() {
        let dir = tree(&[
            ("shell.qml", "import qs.services\nimport \"widgets\"\nShellRoot {}\n"),
            ("services/Audio.qml", "pragma Singleton\nimport QtQuick\nQtObject {}\n"),
            ("services/Helper.qml", "//@ pragma Internal\nQtObject {}\n"),
            ("services/lower.qml", "QtObject {}\n"),
            ("widgets/Bar.qml", "import qs.services\nItem {}\n"),
            ("assets/icon.svg", "<svg/>"),
        ]);
        let scan = Scan::new(&dir.join("shell.qml")).unwrap();
        let root = dir.canonicalize().unwrap();
        assert_eq!(
            scan.intercepts[&root.join("services/qmldir")],
            "module qs.services\nsingleton Audio 1.0 Audio.qml\ninternal Helper 1.0 Helper.qml\n"
        );
        assert_eq!(scan.intercepts[&root.join("widgets/qmldir")], "module qs.widgets\nBar 1.0 Bar.qml\n");
        assert_eq!(scan.intercepts[&root.join("qmldir")], "module qs\n");

        let vfs = dir.join("../").canonicalize().unwrap().join(format!("qsrs-vfs-{}", std::process::id()));
        scan.write_vfs(&vfs).unwrap();
        assert!(vfs.join("services/qmldir").is_file());
        assert!(fs::symlink_metadata(vfs.join("services/Audio.qml")).unwrap().file_type().is_symlink());
        assert!(fs::symlink_metadata(vfs.join("assets")).unwrap().file_type().is_symlink());

        // A second write over the same dir keeps what is right and drops what is stale.
        let mtime = |p: &str| fs::symlink_metadata(vfs.join(p)).unwrap().modified().unwrap();
        let before = mtime("services/qmldir");
        fs::write(vfs.join("stale.qml"), "").unwrap();
        fs::create_dir_all(vfs.join("gone/deep")).unwrap();
        fs::write(vfs.join("widgets/qmldir"), "module old\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        scan.write_vfs(&vfs).unwrap();
        assert_eq!(mtime("services/qmldir"), before);
        assert!(!vfs.join("stale.qml").exists() && !vfs.join("gone").exists());
        assert_eq!(fs::read_to_string(vfs.join("widgets/qmldir")).unwrap(), "module qs.widgets\nBar 1.0 Bar.qml\n");
        fs::remove_dir_all(&vfs).unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn mirrors_plugins_linked_into_modules() {
        // `inir hub` links a family at modules/<id> to its own folder outside the shell.
        let store = tree(&[
            ("fam/Panel.qml", "import QtQuick\nimport \"root:services\"\nItem {}\n"),
            ("fam/parts/Card.qml", "import qs.modules.fam\nItem {}\n"),
            ("fam/parts/loaded.qml", "import \"root:\"\nItem {}\n"),
        ]);
        let dir = tree(&[
            ("shell.qml", "import QtQuick\nShellRoot {}\n"),
            ("HubScan.qml", "import qs.modules.fam\nimport qs.modules.fam.parts\nQtObject {}\n"),
            ("services/Audio.qml", "QtObject {}\n"),
            ("modules/shared/Base.qml", "Item {}\n"),
        ]);
        std::os::unix::fs::symlink(store.join("fam"), dir.join("modules/fam")).unwrap();
        std::os::unix::fs::symlink(&store, dir.join("dev")).unwrap();
        let scan = Scan::new(&dir.join("shell.qml")).unwrap();
        let root = dir.canonicalize().unwrap();
        assert_eq!(scan.intercepts[&root.join("modules/fam/qmldir")], "module qs.modules.fam\nPanel 1.0 Panel.qml\n");
        assert_eq!(scan.intercepts[&root.join("modules/fam/Panel.qml")], "import QtQuick\nimport \"../../services\"\nItem {}\n");
        assert_eq!(scan.intercepts[&root.join("modules/fam/parts/loaded.qml")], "import \"../../../\"\nItem {}\n");
        assert!(!scan.intercepts.keys().any(|k| k.starts_with(root.join("dev"))));

        let vfs = dir.join("../").canonicalize().unwrap().join(format!("qsrs-vfs-link-{}", std::process::id()));
        scan.write_vfs(&vfs).unwrap();
        assert!(fs::symlink_metadata(vfs.join("modules/fam")).unwrap().is_dir());
        assert!(vfs.join("modules/fam/parts/qmldir").is_file());
        assert!(fs::symlink_metadata(vfs.join("dev")).unwrap().file_type().is_symlink());
        fs::remove_dir_all(&vfs).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        fs::remove_dir_all(&store).unwrap();
    }
}

