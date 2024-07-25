// Copyright 2018-2024 the Deno authors. MIT license.

use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

pub struct SetWorkspaceConfigOptions {
  pub config: WorkspaceConfig,
  /// Maintains deno.json dependencies and workspace config
  /// regardless of the `config` options provided.
  ///
  /// Ex. the CLI sets this to `true` when someone runs a
  /// one-off script with `--no-config`.
  pub no_config: bool,
  /// Maintains package.json dependencies regardless of the
  /// `config` options provided.
  ///
  /// Ex. the CLI sets this to `true` when someone runs a
  /// one-off script with `--no-npm`.
  pub no_npm: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceConfig {
  #[serde(flatten)]
  pub root: WorkspaceMemberConfig,
  #[serde(default)]
  pub members: BTreeMap<String, WorkspaceMemberConfig>,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceMemberConfig {
  #[serde(default)]
  pub dependencies: BTreeSet<String>,
  #[serde(default)]
  pub package_json_deps: BTreeSet<String>,
}

#[derive(
  Debug, Default, Clone, Serialize, Deserialize, Hash, PartialEq, PartialOrd,
)]
#[serde(rename_all = "camelCase")]
pub struct LockfilePackageJsonContent {
  #[serde(default)]
  #[serde(skip_serializing_if = "BTreeSet::is_empty")]
  pub dependencies: BTreeSet<String>,
}

#[derive(
  Debug, Default, Clone, Serialize, Deserialize, Hash, PartialEq, PartialOrd,
)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceMemberConfigContent {
  #[serde(skip_serializing_if = "BTreeSet::is_empty")]
  #[serde(default)]
  pub dependencies: BTreeSet<String>,
  #[serde(skip_serializing_if = "LockfilePackageJsonContent::is_empty")]
  #[serde(default)]
  pub package_json: LockfilePackageJsonContent,
}

#[derive(
  Debug, Default, Clone, Serialize, Deserialize, Hash, PartialEq, PartialOrd,
)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceConfigContent {
  #[serde(default, flatten)]
  pub root: WorkspaceMemberConfigContent,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  #[serde(default)]
  pub members: BTreeMap<String, WorkspaceMemberConfigContent>,
}

impl LockfilePackageJsonContent {
  pub fn is_empty(&self) -> bool {
    self.dependencies.is_empty()
  }
}

impl WorkspaceMemberConfigContent {}

impl WorkspaceConfigContent {
  pub fn is_empty(&self) -> bool {
    self.root.is_empty() && self.members.is_empty()
  }

  // TODO: This wasnt pub before
  pub fn get_all_dep_reqs(&self) -> impl Iterator<Item = &String> {
    self
      .root
      .all_dependencies()
      .chain(self.members.values().flat_map(|m| m.all_dependencies()))
  }
}

impl PartialEq<WorkspaceMemberConfigContent> for WorkspaceMemberConfig {
  /// Compare the dependencies of a workspace member config to the deps of a workspace member config content
  fn eq(&self, content: &WorkspaceMemberConfigContent) -> bool {
    self.dependencies == content.dependencies
      && self.package_json_deps == content.package_json.dependencies
  }
}
impl PartialEq<WorkspaceMemberConfig> for WorkspaceMemberConfigContent {
  fn eq(&self, content: &WorkspaceMemberConfig) -> bool {
    content == self
  }
}

impl From<WorkspaceMemberConfigContent> for WorkspaceMemberConfig {
  fn from(value: WorkspaceMemberConfigContent) -> Self {
    Self {
      dependencies: value.dependencies,
      package_json_deps: value.package_json.dependencies,
    }
  }
}
impl From<&WorkspaceMemberConfigContent> for WorkspaceMemberConfig {
  fn from(value: &WorkspaceMemberConfigContent) -> Self {
    Self {
      dependencies: value.dependencies.clone(),
      package_json_deps: value.package_json.dependencies.clone(),
    }
  }
}
impl From<WorkspaceMemberConfig> for WorkspaceMemberConfigContent {
  fn from(value: WorkspaceMemberConfig) -> Self {
    Self {
      dependencies: value.dependencies,
      package_json: LockfilePackageJsonContent {
        dependencies: value.package_json_deps,
      },
    }
  }
}
impl From<&WorkspaceMemberConfig> for WorkspaceMemberConfigContent {
  fn from(value: &WorkspaceMemberConfig) -> Self {
    Self {
      dependencies: value.dependencies.clone(),
      package_json: LockfilePackageJsonContent {
        dependencies: value.package_json_deps.clone(),
      },
    }
  }
}

impl WorkspaceConfig {
  /// Create a new config that preserves some dependencies based on the current state and the [SetWorkspaceConfigOptions]
  pub fn new(
    set_options: SetWorkspaceConfigOptions,
    current_config: &WorkspaceConfigContent,
  ) -> Self {
    let mut config = set_options.config;

    let no_npm = set_options.no_npm || set_options.no_config;
    let no_config = set_options.no_config;

    config.root = WorkspaceMemberConfig {
      dependencies: match no_config {
        true => current_config.root.dependencies.clone(),
        false => config.root.dependencies,
      },
      package_json_deps: match no_npm {
        true => current_config.root.package_json.dependencies.clone(),
        false => config.root.package_json_deps,
      },
    };

    config.members = config
      .members
      .into_iter()
      .map(|(key, value)| {
        let current_member = current_config
          .members
          .get(&key)
          .cloned()
          .unwrap_or_default();
        let new_config = WorkspaceMemberConfig {
          dependencies: match no_config {
            true => current_member.dependencies.clone(),
            false => value.dependencies,
          },
          package_json_deps: match no_npm {
            true => current_member.package_json.dependencies.clone(),
            false => value.package_json_deps,
          },
        };
        (key, new_config)
      })
      .collect();

    if no_config {
      // Preserve the current config members
      config.members.extend(
        current_config
          .members
          .iter()
          .filter(|(key, _)| !config.members.contains_key(*key))
          .collect::<Vec<_>>()
          .into_iter()
          .map(|(k, v)| (k.clone(), v.into())),
      );
    }

    config
  }
}

impl WorkspaceConfigContent {
  /// Apply the config to this workspace member
  pub fn update(&mut self, config: WorkspaceConfig) {
    self.root.update(config.root);
    self.members = config
      .members
      .into_iter()
      .map(|(member_name, new_member_config)| {
        // Reuse an old member, if it exists
        let mut new_member =
          self.members.remove(&member_name).unwrap_or_default();
        new_member.update(new_member_config);
        (member_name, new_member)
      })
      .collect();
  }
}

impl WorkspaceMemberConfigContent {
  /// Check if this workspace member has any dependencies
  pub fn is_empty(&self) -> bool {
    self.dependencies.is_empty() && self.package_json.is_empty()
  }

  /// Get an iterator over all dependencies of this workspace member
  pub fn all_dependencies(&self) -> impl Iterator<Item = &String> {
    self
      .package_json
      .dependencies
      .iter()
      .chain(self.dependencies.iter())
  }

  /// Apply the config to this workspace member
  pub fn update(&mut self, config: WorkspaceMemberConfig) {
    if config.dependencies != self.dependencies {
      self.dependencies = config.dependencies;
    }

    if config.package_json_deps != self.package_json.dependencies {
      self.package_json.dependencies = config.package_json_deps;
    }
  }
}
