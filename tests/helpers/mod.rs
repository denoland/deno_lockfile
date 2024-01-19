use std::iter::Peekable;
use std::path::Path;
use std::path::PathBuf;

pub struct ConfigChangeSpec {
  pub path: PathBuf,
  pub original_text: SpecFile,
  pub change: SpecFile,
  pub output: SpecFile,
}

impl ConfigChangeSpec {
  pub fn collect_in_dir(dir_path: &Path) -> Vec<ConfigChangeSpec> {
    collect_files_in_dir_recursive(dir_path)
      .into_iter()
      .map(|file| ConfigChangeSpec::parse(file.path.clone(), &file.text))
      .collect()
  }

  fn parse(path: PathBuf, text: &str) -> Self {
    fn take_next<'a>(
      lines: &mut Peekable<impl Iterator<Item = &'a str>>,
    ) -> String {
      let mut result = String::new();
      while let Some(line) = lines.next() {
        result.push_str(line);
        result.push_str("\n");
        if let Some(next_line) = lines.peek() {
          if next_line.starts_with('#') {
            break;
          }
        }
      }
      result
    }

    let mut lines = text.split('\n').peekable();
    let original_text = SpecFile {
      title: lines.next().unwrap().to_string(),
      text: take_next(&mut lines),
    };
    let change = SpecFile {
      title: lines.next().unwrap().to_string(),
      text: take_next(&mut lines),
    };
    let output = SpecFile {
      title: lines.next().unwrap().to_string(),
      text: take_next(&mut lines),
    };
    assert!(lines.next().is_none());
    Self {
      path,
      original_text,
      change,
      output,
    }
  }

  pub fn emit(&self) -> String {
    let mut text = String::new();
    text.push_str(&self.original_text.emit());
    text.push_str("\n");
    text.push_str(&self.change.emit());
    text.push_str("\n");
    text.push_str(&self.output.emit());
    text.push_str("\n");
    text
  }
}

pub struct SpecFile {
  pub title: String,
  pub text: String,
}

impl SpecFile {
  pub fn emit(&self) -> String {
    format!("# {}\n{}\n", self.title, self.text)
  }
}

struct CollectedFile {
  pub path: PathBuf,
  pub text: String,
}

fn collect_files_in_dir_recursive(path: &Path) -> Vec<CollectedFile> {
  let mut result = Vec::new();

  for entry in path.read_dir().unwrap().flatten() {
    let entry_path = entry.path();
    if entry_path.is_file() {
      let text = std::fs::read_to_string(&entry_path).unwrap();
      result.push(CollectedFile {
        path: entry_path,
        text,
      });
    } else {
      result.extend(collect_files_in_dir_recursive(&entry_path));
    }
  }

  result
}
