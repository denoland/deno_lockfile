use std::iter::Peekable;
use std::path::Path;
use std::path::PathBuf;

pub struct ChangeAndOutput {
  pub change: SpecFile,
  pub output: SpecFile,
}

pub struct ConfigChangeSpec {
  pub path: PathBuf,
  pub original_text: SpecFile,
  pub change_and_outputs: Vec<ChangeAndOutput>,
}

impl ConfigChangeSpec {
  pub fn collect_in_dir(dir_path: &Path) -> Vec<ConfigChangeSpec> {
    let files = collect_files_in_dir_recursive(dir_path);
    let only_files = files
      .iter()
      .filter(|file| {
        file.path.to_string_lossy().to_lowercase().contains("_only")
      })
      .cloned()
      .collect::<Vec<_>>();
    let files = if only_files.is_empty() {
      files
    } else {
      only_files
    };
    files
      .into_iter()
      .map(|file| ConfigChangeSpec::parse(file.path.clone(), &file.text))
      .collect()
  }

  fn parse(path: PathBuf, text: &str) -> Self {
    fn take_header<'a>(lines: &mut impl Iterator<Item = &'a str>) -> String {
      lines
        .next()
        .unwrap()
        .strip_prefix("# ")
        .unwrap()
        .to_string()
    }

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
      title: take_header(&mut lines),
      text: take_next(&mut lines),
    };
    let mut change_and_outputs = Vec::new();
    while lines.peek().is_some() {
      let change = SpecFile {
        title: take_header(&mut lines),
        text: take_next(&mut lines),
      };
      let output = SpecFile {
        title: take_header(&mut lines),
        text: take_next(&mut lines),
      };
      change_and_outputs.push(ChangeAndOutput { change, output });
    }
    Self {
      path,
      original_text,
      change_and_outputs,
    }
  }

  pub fn emit(&self) -> String {
    let mut text = String::new();
    text.push_str(&self.original_text.emit());
    for (i, change_and_output) in self.change_and_outputs.iter().enumerate() {
      if i > 0 {
        text.push('\n');
      }
      text.push_str(&change_and_output.change.emit());
      text.push_str(&change_and_output.output.emit());
    }
    text
  }
}

pub struct SpecFile {
  pub title: String,
  pub text: String,
}

impl SpecFile {
  pub fn emit(&self) -> String {
    format!("# {}\n{}", self.title, self.text)
  }
}

#[derive(Clone)]
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
