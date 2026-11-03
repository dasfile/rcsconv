//! Обход входов: файлы, каталоги, маски — и имя результата.

use std::path::{Path, PathBuf};

/// Совпадает ли расширение файла (регистр не важен).
pub fn has_suffix(path: &Path, suffix: &str) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| format!(".{value}").eq_ignore_ascii_case(suffix))
}

/// Имя выхода: то же имя с расширением `.rcs` (и, если задан, в `out_dir`).
pub fn default_target(source: &Path, out_dir: Option<&Path>) -> PathBuf {
    let name = source
        .file_stem()
        .map(|stem| format!("{}.rcs", stem.to_string_lossy()))
        .unwrap_or_else(|| "output.rcs".to_string());
    match out_dir {
        Some(dir) => dir.join(name),
        None => source.with_file_name(name),
    }
}

/// Развернуть аргументы CLI в список входных файлов с нужным расширением.
///
/// Принимает файлы, каталоги (все файлы с расширением внутри) и маски.
pub fn expand_sources(patterns: &[String], suffixes: &[&str], recursive: bool) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for pattern in patterns {
        let path = Path::new(pattern);
        if path.is_dir() {
            collect_dir(path, suffixes, recursive, &mut found);
            continue;
        }
        if pattern.contains('*') || pattern.contains('?') {
            let mut matches = glob(pattern);
            matches.sort();
            if matches.is_empty() {
                found.push(path.to_path_buf());
            } else {
                found.extend(matches);
            }
            continue;
        }
        found.push(path.to_path_buf());
    }

    let mut unique: Vec<PathBuf> = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    for path in found {
        let key = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        unique.push(path);
    }
    unique
}

/// Файлы каталога с нужными расширениями (сортировка по пути).
pub fn collect_dir(dir: &Path, suffixes: &[&str], recursive: bool, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = Vec::new();
    let mut dirs: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path);
        } else if suffixes.iter().any(|suffix| has_suffix(&path, suffix)) {
            files.push(path);
        }
    }
    files.sort();
    out.extend(files);
    if recursive {
        dirs.sort();
        for sub in dirs {
            collect_dir(&sub, suffixes, true, out);
        }
    }
}

/// Маска в одном компоненте пути: `*` — любая подстрока, `?` — один символ.
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    match pattern.chars().next() {
        None => text.is_empty(),
        Some('*') => {
            let rest = &pattern[1..];
            (0..=text.len())
                .filter(|index| text.is_char_boundary(*index))
                .any(|index| wildcard_match(rest, &text[index..]))
        }
        Some('?') => match text.chars().next() {
            Some(first) => wildcard_match(&pattern[1..], &text[first.len_utf8()..]),
            None => false,
        },
        Some(other) => match text.chars().next() {
            Some(first) if first == other => {
                wildcard_match(&pattern[other.len_utf8()..], &text[first.len_utf8()..])
            }
            _ => false,
        },
    }
}

/// Развернуть маску в список существующих файлов (каталог + имя с маской).
pub fn glob(pattern: &str) -> Vec<PathBuf> {
    let path = Path::new(pattern);
    let dir = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let Some(mask) = path.file_name().and_then(|name| name.to_str()) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| wildcard_match(mask, name))
        })
        .map(|entry| entry.path())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards() {
        assert!(wildcard_match("*.laz", "level_7.laz"));
        assert!(!wildcard_match("*.laz", "level_7.las"));
        assert!(wildcard_match("level_?", "level_7"));
        assert!(wildcard_match("*", "anything"));
        assert!(!wildcard_match("a*b", "ab_c"));
        assert!(wildcard_match("a*b", "aXXb"));
    }

    #[test]
    fn target_name() {
        assert_eq!(
            default_target(Path::new("scan.laz"), None),
            PathBuf::from("scan.rcs")
        );
        assert_eq!(
            default_target(Path::new("scan.laz"), Some(Path::new("out"))),
            PathBuf::from("out").join("scan.rcs")
        );
    }

    #[test]
    fn suffixes_ignore_case() {
        assert!(has_suffix(Path::new("a.LAZ"), ".laz"));
        assert!(!has_suffix(Path::new("a.las"), ".laz"));
    }

    #[test]
    fn directory_is_expanded_and_deduplicated() {
        let dir = std::env::temp_dir().join(format!("rcs-conv-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).expect("каталог");
        for name in ["a.laz", "b.LAZ", "c.pts", "notes.txt"] {
            std::fs::write(dir.join(name), b"x").expect("файл");
        }
        std::fs::write(dir.join("sub").join("d.laz"), b"x").expect("файл");

        let patterns = vec![dir.to_string_lossy().into_owned()];
        let flat = expand_sources(&patterns, &[".las", ".laz"], false);
        assert_eq!(flat.len(), 2, "{flat:?}");
        assert!(flat.iter().all(|path| has_suffix(path, ".laz")));

        let deep = expand_sources(&patterns, &[".las", ".laz"], true);
        assert_eq!(deep.len(), 3, "{deep:?}");

        let twice = vec![
            dir.join("a.laz").to_string_lossy().into_owned(),
            dir.join("a.laz").to_string_lossy().into_owned(),
        ];
        assert_eq!(expand_sources(&twice, &[".laz"], false).len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
