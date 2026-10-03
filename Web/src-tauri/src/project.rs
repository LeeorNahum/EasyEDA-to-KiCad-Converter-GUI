//! The KiCad project an output folder belongs to, and paths as that
//! project's library tables and footprints write them.

use std::fs;
use std::path::{Component, Path, PathBuf};

/// A KiCad project: a folder holding a `.kicad_pro` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub folder: PathBuf,
    /// The project file. When the folder holds several, the first by name.
    pub file: PathBuf,
}

/// The project `folder` is in: the nearest folder, `folder` itself or one
/// above it, that holds a `.kicad_pro` file.
pub fn find(folder: &Path) -> Option<Project> {
    let folder = std::path::absolute(folder).ok()?;
    folder.ancestors().find_map(|candidate| {
        let mut files: Vec<PathBuf> = fs::read_dir(candidate)
            .ok()?
            .flatten()
            .map(|entry| entry.path())
            // Windows file names ignore case, and so does KiCad there.
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("kicad_pro"))
                    && path.is_file()
            })
            .collect();
        files.sort();
        let file = files.into_iter().next()?;
        Some(Project {
            folder: candidate.to_path_buf(),
            file,
        })
    })
}

impl Project {
    /// `path` as `${KIPRJMOD}/...`, KiCad's name for the project folder, with
    /// forward slashes as KiCad writes them. `None` when `path` is not inside
    /// the project folder.
    pub fn reference(&self, path: &Path) -> Option<String> {
        let path = lexical(&std::path::absolute(path).ok()?);
        let inside = path.strip_prefix(lexical(&self.folder)).ok()?;
        let parts: Vec<String> = inside
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        Some(if parts.is_empty() {
            "${KIPRJMOD}".to_string()
        } else {
            format!("${{KIPRJMOD}}/{}", parts.join("/"))
        })
    }

    /// Where a library table's `uri` points: a full path, a path relative to
    /// the project, or one through path variables, `${NAME}` or `$(NAME)`.
    /// `None` when it uses a variable that is not set.
    pub fn resolve(&self, uri: &str) -> Option<PathBuf> {
        self.resolve_with(uri, &path_variable)
    }

    /// `resolve`, reading every variable but `KIPRJMOD` from `variable`.
    fn resolve_with(
        &self,
        uri: &str,
        variable: &dyn Fn(&str) -> Option<String>,
    ) -> Option<PathBuf> {
        let mut expanded = String::new();
        let mut rest = uri;
        while let Some(at) = [rest.find("${"), rest.find("$(")]
            .into_iter()
            .flatten()
            .min()
        {
            let close = if rest[at..].starts_with("${") {
                '}'
            } else {
                ')'
            };
            let length = rest[at + 2..].find(close)?;
            let name = &rest[at + 2..at + 2 + length];
            expanded.push_str(&rest[..at]);
            if name == "KIPRJMOD" {
                expanded.push_str(&self.folder.to_string_lossy());
            } else {
                expanded.push_str(&variable(name)?);
            }
            rest = &rest[at + 2 + length + 1..];
        }
        expanded.push_str(rest);
        Some(self.folder.join(expanded))
    }

    /// Whether KiCad has the project open. KiCad holds `~<name>.kicad_pro.lck`
    /// beside the project file while the project is open.
    pub fn is_open(&self) -> bool {
        let Some(name) = self.file.file_name() else {
            return false;
        };
        let mut lock = std::ffi::OsString::from("~");
        lock.push(name);
        lock.push(".lck");
        self.folder.join(lock).is_file()
    }
}

/// A path variable as KiCad sees it: from the system environment, which
/// KiCad lets override its own settings, or else from Preferences >
/// Configure Paths, saved in KiCad's settings folder. That folder is
/// `KICAD_CONFIG_HOME` when it is set, as KiCad reads it, and otherwise
/// `kicad` in the user's settings folder.
fn path_variable(name: &str) -> Option<String> {
    if let Ok(value) = std::env::var(name) {
        return Some(value);
    }
    let settings = match std::env::var_os("KICAD_CONFIG_HOME") {
        Some(home) if !home.is_empty() => PathBuf::from(home),
        _ => dirs::config_dir()?.join("kicad"),
    };
    configured(&settings, name)
}

/// A path variable from the `kicad_common.json` of each KiCad version in
/// `settings`, such as `9.0` and `10.0`. Each version keeps its own, and
/// which one will open the project is not known here, so the variable has
/// a value only when every version sets it, and to the same thing.
fn configured(settings: &Path, name: &str) -> Option<String> {
    let mut values = fs::read_dir(settings).ok()?.flatten().filter_map(|entry| {
        let text = fs::read_to_string(entry.path().join("kicad_common.json")).ok()?;
        let common: serde_json::Value = serde_json::from_str(&text).ok()?;
        Some(
            common["environment"]["vars"][name]
                .as_str()
                .map(str::to_string),
        )
    });
    let value = values.next()??;
    values
        .all(|other| other.as_ref() == Some(&value))
        .then_some(value)
}

/// Whether two paths name the same file or folder.
pub fn same_place(a: &Path, b: &Path) -> bool {
    if let (Ok(a), Ok(b)) = (fs::canonicalize(a), fs::canonicalize(b)) {
        return a == b;
    }
    let text = |path: &Path| {
        std::path::absolute(path).map(|path| {
            let path = lexical(&path).to_string_lossy().replace('\\', "/");
            // Windows file names ignore case.
            if cfg!(windows) {
                path.to_lowercase()
            } else {
                path
            }
        })
    };
    matches!((text(a), text(b)), (Ok(a), Ok(b)) if a == b)
}

/// `path` with `.` and `..` worked out without asking the file system.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("easyeda-project-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(folder.join("lib").join("Part_C1")).unwrap();
        folder
    }

    #[test]
    fn the_nearest_project_folder_is_found() {
        let folder = scratch("find");
        let inner = folder.join("lib").join("Part_C1");
        assert_eq!(find(&inner), None);
        fs::write(folder.join("Board.KICAD_PRO"), "{}").unwrap();
        assert_eq!(find(&inner).unwrap().file, folder.join("Board.KICAD_PRO"));
        fs::write(folder.join("Another.kicad_pro"), "{}").unwrap();
        let project = find(&inner).unwrap();
        assert_eq!(project.folder, folder);
        assert_eq!(project.file, folder.join("Another.kicad_pro"));
        assert_eq!(
            project.reference(&inner.join("Part_C1.3dshapes")).unwrap(),
            "${KIPRJMOD}/lib/Part_C1/Part_C1.3dshapes"
        );
        assert_eq!(project.reference(&folder).unwrap(), "${KIPRJMOD}");
        assert_eq!(project.reference(&std::env::temp_dir()), None);
        assert!(!project.is_open());
        fs::write(folder.join("~Another.kicad_pro.lck"), "").unwrap();
        assert!(project.is_open());
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_path_variable_has_a_value_only_when_every_version_agrees() {
        let settings = scratch("settings");
        let set = |version: &str, value: &str| {
            fs::create_dir_all(settings.join(version)).unwrap();
            let common = serde_json::json!({ "environment": { "vars": { "TEAM_LIB": value } } });
            fs::write(
                settings.join(version).join("kicad_common.json"),
                common.to_string(),
            )
            .unwrap();
        };
        assert_eq!(configured(&settings, "TEAM_LIB"), None);
        set("10.0", "C:/Project/lib");
        assert_eq!(
            configured(&settings, "TEAM_LIB").as_deref(),
            Some("C:/Project/lib")
        );
        assert_eq!(configured(&settings, "OTHER"), None);
        // A version that leaves it unset could not follow the path.
        fs::create_dir_all(settings.join("9.0")).unwrap();
        fs::write(settings.join("9.0").join("kicad_common.json"), "{}").unwrap();
        assert_eq!(configured(&settings, "TEAM_LIB"), None);
        set("9.0", "C:/Project/lib");
        assert!(configured(&settings, "TEAM_LIB").is_some());
        set("9.0", "C:/DifferentLibrary");
        assert_eq!(configured(&settings, "TEAM_LIB"), None);
        fs::remove_dir_all(&settings).unwrap();
    }

    #[test]
    fn table_paths_resolve_against_the_project() {
        let folder = scratch("resolve");
        let project = Project {
            folder: folder.clone(),
            file: folder.join("Board.kicad_pro"),
        };
        let library = folder.join("lib").join("Part_C1");
        for uri in [
            "${KIPRJMOD}/lib/Part_C1",
            "$(KIPRJMOD)/lib/Part_C1/",
            "lib/Part_C1",
            "${KIPRJMOD}/lib/other/../Part_C1",
        ] {
            let resolved = project.resolve(uri).unwrap();
            assert!(same_place(&resolved, &library), "{uri}");
        }
        let full = library.to_string_lossy().replace('\\', "/");
        assert!(same_place(&project.resolve(&full).unwrap(), &library));
        let team = |name: &str| {
            (name == "TEAM_LIB").then(|| folder.join("lib").to_string_lossy().into_owned())
        };
        assert!(same_place(
            &project.resolve_with("${TEAM_LIB}/Part_C1", &team).unwrap(),
            &library
        ));
        assert!(project.resolve_with("${OTHER}/Part_C1", &team).is_none());
        assert!(project.resolve_with("${KIPRJMOD/lib", &team).is_none());
        assert!(!same_place(
            &project.resolve("lib/Part_C2").unwrap(),
            &library
        ));
        assert!(
            same_place(
                &project.resolve("LIB/missing.kicad_sym").unwrap(),
                &folder.join("lib").join("missing.kicad_sym")
            ) == cfg!(windows)
        );
        fs::remove_dir_all(&folder).unwrap();
    }
}
