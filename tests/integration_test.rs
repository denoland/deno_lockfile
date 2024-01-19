use deno_lockfile::Lockfile;
use deno_lockfile::SetWorkspaceConfigOptions;
use std::path::PathBuf;

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
  for mut spec in specs {
    eprintln!("Looking at {}...", spec.path.display());
    let mut config_file = Lockfile::with_lockfile_content(
      spec.path.with_extension(".lock"),
      &spec.original_text.text,
      false,
    )
    .unwrap();
    config_file.set_workspace_config(SetWorkspaceConfigOptions {
      config: serde_json::from_str(&spec.change.text).unwrap(),
      nv_to_jsr_url,
    });
    let expected_text = spec.output.text.clone();
    let actual_text = config_file.as_json_string();
    if std::env::var("UPDATE") == Ok("1".to_string()) {
      spec.output.text = actual_text;
      std::fs::write(&spec.path, spec.emit()).unwrap();
    } else {
      assert_eq!(
        actual_text,
        expected_text,
        "Failed for: {}",
        spec.path.display()
      );
    }
  }
}
