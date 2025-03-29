// Copyright 2018-2024 the Deno authors. MIT license.

use std::collections::HashSet;
use std::path::PathBuf;

use deno_lockfile::WorkspaceConfig;
use deno_lockfile::WorkspaceMemberConfig;

use deno_lockfile::Lockfile;
use deno_lockfile::SetWorkspaceConfigOptions;
use deno_semver::jsr::JsrDepPackageReq;

#[test]
fn adding_workspace_does_not_cause_content_changes() {
  // should maintain the has_content_changed flag when lockfile empty
  {
    let mut lockfile =
      Lockfile::new_empty(PathBuf::from("./deno.lock"), true, true);

    assert!(!lockfile.has_content_changed);
    lockfile.set_workspace_config(SetWorkspaceConfigOptions {
      no_config: false,
      no_npm: false,
      config: WorkspaceConfig {
        root: WorkspaceMemberConfig {
          dependencies: HashSet::from([JsrDepPackageReq::from_str(
            "jsr:@scope/package",
          )
          .unwrap()]),
          package_json_deps: Default::default(),
        },
        members: Default::default(),
        patches: Default::default(),
      },
    });
    assert!(!lockfile.has_content_changed); // should not have changed
  }

  // should maintain has_content_changed flag when true and lockfile is empty
  {
    let mut lockfile =
      Lockfile::new_empty(PathBuf::from("./deno.lock"), true, true);
    lockfile.has_content_changed = true;
    lockfile.set_workspace_config(SetWorkspaceConfigOptions {
      no_config: false,
      no_npm: false,
      config: WorkspaceConfig {
        root: WorkspaceMemberConfig {
          dependencies: HashSet::from([JsrDepPackageReq::from_str(
            "jsr:@scope/package2",
          )
          .unwrap()]),
          package_json_deps: Default::default(),
        },
        members: Default::default(),
        patches: Default::default(),
      },
    });
    assert!(lockfile.has_content_changed);
  }

  // should not maintain the has_content_changed flag when lockfile is not empty
  {
    let mut lockfile =
      Lockfile::new_empty(PathBuf::from("./deno.lock"), true, true);
    lockfile
      .content
      .redirects
      .insert("a".to_string(), "b".to_string());

    assert!(!lockfile.has_content_changed);
    lockfile.set_workspace_config(SetWorkspaceConfigOptions {
      no_config: false,
      no_npm: false,
      config: WorkspaceConfig {
        root: WorkspaceMemberConfig {
          dependencies: HashSet::from([JsrDepPackageReq::from_str(
            "jsr:@scope/package",
          )
          .unwrap()]),
          package_json_deps: Default::default(),
        },
        members: Default::default(),
        patches: Default::default(),
      },
    });
    assert!(lockfile.has_content_changed); // should have changed since lockfile was not empty
  }
}
