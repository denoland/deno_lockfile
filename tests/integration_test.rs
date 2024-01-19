use pretty_assertions::assert_eq;
use std::path::PathBuf;

use deno_lockfile::Lockfile;
use deno_lockfile::SetWorkspaceConfigOptions;

use helpers::ConfigChangeSpec;

mod helpers;

fn nv_to_jsr_url(nv: &str) -> Option<String> {
  // very hacky, but good enough for tests
  let path = format!("@{}", nv[1..].replace("@", "/"));
  Some(format!("https://jsr.io/{}/", path))
}

#[test]
fn config_changes() {
  let specs = ConfigChangeSpec::collect_in_dir(&PathBuf::from(
    "./tests/specs/config_changes",
  ));
  let is_update = std::env::var("UPDATE") == Ok("1".to_string());
  for mut spec in specs {
    eprintln!("Looking at {}...", spec.path.display());
    let mut config_file = Lockfile::with_lockfile_content(
      spec.path.with_extension(".lock"),
      &spec.original_text.text,
      false,
    )
    .unwrap();
    for change_and_output in &mut spec.change_and_outputs {
      config_file.set_workspace_config(SetWorkspaceConfigOptions {
        config: serde_json::from_str(&change_and_output.change.text).unwrap(),
        nv_to_jsr_url,
      });
      let expected_text = change_and_output.output.text.clone();
      let actual_text = config_file.as_json_string();
      if is_update {
        change_and_output.output.text = actual_text;
      } else {
        assert_eq!(
          actual_text.trim(),
          expected_text.trim(),
          "Failed for: {} - {}",
          spec.path.display(),
          change_and_output.change.title,
        );
      }
    }
    if is_update {
      std::fs::write(&spec.path, spec.emit()).unwrap();
    }
  }
}
