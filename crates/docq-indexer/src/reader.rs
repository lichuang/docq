use std::path::{Path, PathBuf};

use docq_core::{ParseError, Result};
use glob::Pattern;
use walkdir::WalkDir;

pub struct DocumentSource {
  pub path: PathBuf,
  pub content: String,
}

pub struct TextReader {
  extensions: Vec<String>,
  ignore_patterns: Vec<Pattern>,
}

impl Default for TextReader {
  fn default() -> Self {
    Self::new()
  }
}

impl TextReader {
  pub fn new() -> Self {
    Self::with_extensions(&["txt", "md"])
  }

  pub fn with_extensions(extensions: &[&str]) -> Self {
    let ignore_patterns = [".git", "target", "node_modules"].iter().filter_map(|p| Pattern::new(p).ok()).collect();
    Self {
      extensions: extensions.iter().map(|s| s.to_string()).collect(),
      ignore_patterns,
    }
  }

  pub fn read_dir(&self, path: &Path, recursive: bool) -> Result<Vec<DocumentSource>> {
    let mut docs = Vec::new();
    let walker = if recursive {
      WalkDir::new(path)
    } else {
      WalkDir::new(path).max_depth(1)
    };

    for entry in walker.into_iter().filter_entry(|e| e.depth() == 0 || !self.is_ignored(e.path())) {
      let entry = match entry {
        Ok(e) => e,
        Err(_) => continue,
      };
      if !entry.file_type().is_file() {
        continue;
      }
      let p = entry.path();
      if !self.has_valid_extension(p) {
        continue;
      }
      match std::fs::read_to_string(p) {
        Ok(content) => docs.push(DocumentSource {
          path: p.to_path_buf(),
          content,
        }),
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => continue,
        Err(e) => return Err(ParseError::Other(format!("read {}: {e}", p.display())).into()),
      }
    }
    Ok(docs)
  }

  fn has_valid_extension(&self, path: &Path) -> bool {
    path
      .extension()
      .and_then(|ext| ext.to_str())
      .is_some_and(|ext| self.extensions.iter().any(|e| e == ext))
  }

  fn is_ignored(&self, path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.starts_with('.') || self.ignore_patterns.iter().any(|pat| pat.matches(name))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;
  use tempfile::TempDir;

  #[test]
  fn test_reader_txt_and_md() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("a.txt"), "hello").unwrap();
    fs::write(tmp.path().join("b.md"), "# world").unwrap();
    fs::write(tmp.path().join("c.bin"), "binary").unwrap();

    let reader = TextReader::new();
    let docs = reader.read_dir(tmp.path(), true).unwrap();
    assert_eq!(docs.len(), 2);
    let names: Vec<String> = docs.iter().map(|d| d.path.file_name().unwrap().to_str().unwrap().to_string()).collect();
    assert!(names.contains(&"a.txt".to_string()));
    assert!(names.contains(&"b.md".to_string()));
  }

  #[test]
  fn test_reader_ignores_hidden_and_dirs() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("visible.txt"), "ok").unwrap();
    fs::write(tmp.path().join(".hidden.txt"), "hidden").unwrap();

    let git_dir = tmp.path().join(".git");
    fs::create_dir_all(&git_dir).unwrap();
    fs::write(git_dir.join("config"), "git stuff").unwrap();

    let target_dir = tmp.path().join("target");
    fs::create_dir_all(&target_dir).unwrap();
    fs::write(target_dir.join("build.txt"), "build output").unwrap();

    let reader = TextReader::new();
    let docs = reader.read_dir(tmp.path(), true).unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].path.file_name().unwrap(), "visible.txt");
  }

  #[test]
  fn test_reader_recursive() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("top.txt"), "top").unwrap();
    let sub = tmp.path().join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("deep.txt"), "deep").unwrap();

    let reader = TextReader::new();
    let docs_recursive = reader.read_dir(tmp.path(), true).unwrap();
    assert_eq!(docs_recursive.len(), 2);

    let docs_flat = reader.read_dir(tmp.path(), false).unwrap();
    assert_eq!(docs_flat.len(), 1);
  }

  #[test]
  fn test_reader_skips_non_utf8() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("bad.txt");
    let invalid_utf8: &[u8] = &[0xFF, 0xFE, 0xFD];
    fs::write(&path, invalid_utf8).unwrap();
    fs::write(tmp.path().join("good.txt"), "valid").unwrap();

    let reader = TextReader::new();
    let docs = reader.read_dir(tmp.path(), true).unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].path.file_name().unwrap(), "good.txt");
  }
}
