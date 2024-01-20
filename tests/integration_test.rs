use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::PathBuf;

use deno_lockfile::WorkspaceConfig;
use deno_lockfile::WorkspaceMemberConfig;
use pretty_assertions::assert_eq;
use serde;

use deno_lockfile::Lockfile;
use deno_lockfile::SetWorkspaceConfigOptions;

use helpers::ConfigChangeSpec;
use serde::Deserialize;
use serde::Serialize;

mod helpers;

fn nv_to_jsr_url(nv: &str) -> Option<String> {
  // very hacky, but good enough for tests
  let path = format!("@{}", nv[1..].replace("@", "/"));
  Some(format!("https://jsr.io/{}/", path))
}

#[test]
fn config_changes() {
  #[derive(Debug, Default, Clone, Serialize, Deserialize, Hash)]
  #[serde(rename_all = "camelCase")]
  struct LockfilePackageJsonContent {
    #[serde(default)]
    dependencies: BTreeSet<String>,
  }

  #[derive(Debug, Default, Clone, Serialize, Deserialize, Hash)]
  #[serde(rename_all = "camelCase")]
  struct WorkspaceMemberConfigContent {
    #[serde(default)]
    dependencies: Option<BTreeSet<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    package_json: Option<LockfilePackageJsonContent>,
  }

  #[derive(Debug, Default, Clone, Serialize, Deserialize, Hash)]
  #[serde(rename_all = "camelCase")]
  struct WorkspaceConfigContent {
    #[serde(default, flatten)]
    root: WorkspaceMemberConfigContent,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(default)]
    members: BTreeMap<String, WorkspaceMemberConfigContent>,
  }
  impl WorkspaceConfigContent {
    fn into_workspace_config(self) -> WorkspaceConfig {
      WorkspaceConfig {
        root: WorkspaceMemberConfig {
          dependencies: self.root.dependencies,
          package_json_deps: self.root.package_json.map(|p| p.dependencies),
        },
        members: self
          .members
          .into_iter()
          .map(|(k, v)| {
            (
              k,
              WorkspaceMemberConfig {
                dependencies: v.dependencies,
                package_json_deps: v.package_json.map(|p| p.dependencies),
              },
            )
          })
          .collect(),
      }
    }
  }

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
    eprintln!("CONFIG FILE: {:#?}", config_file.content);
    for change_and_output in &mut spec.change_and_outputs {
      // setting the new workspace config should change the has_content_changed flag
      config_file.has_content_changed = false;
      let config = serde_json::from_str::<WorkspaceConfigContent>(
        &change_and_output.change.text,
      )
      .unwrap()
      .into_workspace_config();
      config_file.set_workspace_config(SetWorkspaceConfigOptions {
        config: config.clone(),
        nv_to_jsr_url,
      });
      assert_eq!(
        config_file.has_content_changed,
        !change_and_output.change.title.contains("no change"),
        "Failed for {}",
        change_and_output.change.title,
      );

      // now try resetting it and the flag should remain the same
      config_file.has_content_changed = false;
      config_file.set_workspace_config(SetWorkspaceConfigOptions {
        config: config.clone(),
        nv_to_jsr_url,
      });
      assert!(!config_file.has_content_changed);

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
