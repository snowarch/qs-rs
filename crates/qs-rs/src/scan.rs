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
            if name.starts_with(|c: char| c.is_uppercase()) && name.ends_with(".qml") {
                if let Some(info) = self.scan_file(&dir.join(name)) {
                    entries.push((name.clone(), info));
                }
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
                } else if let (Some(a), Some(b)) = (line.find('"'), line.rfind('"')) {
                    if b > a + 1 {
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
            if let Ok(dir) = import.canonicalize() {
                if dir.is_dir() {
                    self.scan_dir(&dir);
                }
            }
        }
        Some(info)
    }

    /// Mirror the shell into `vfs`: real dirs for scanned dirs, symlinks for everything else,
    /// synthesized qmldir files written in place. Returns the path of the mirrored root file.
    pub fn write_vfs(&self, vfs: &Path) -> io::Result<()> {
        if vfs.exists() {
            fs::remove_dir_all(vfs)?;
        }
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
        for dir in &real {
            let rel = dir.strip_prefix(&self.root).unwrap();
            let out = vfs.join(rel);
            fs::create_dir_all(&out)?;
            for entry in fs::read_dir(dir)?.flatten() {
                let src = entry.path();
                if real.contains(&src) || self.intercepts.contains_key(&src) {
                    continue;
                }
                std::os::unix::fs::symlink(&src, out.join(entry.file_name()))?;
            }
        }
        for (path, content) in &self.intercepts {
            if let Ok(rel) = path.strip_prefix(&self.root) {
                fs::write(vfs.join(rel), content)?;
            }
        }
        Ok(())
    }
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
        fs::remove_dir_all(&vfs).unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }
}

