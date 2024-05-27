// Copyright 2018-2024 the Deno authors. All rights reserved. MIT license.

mod error;
mod graphs;

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

mod transforms;

pub use error::LockfileError as Error;

use crate::graphs::LockfilePackageGraph;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpmPackageLockfileInfo {
  pub serialized_id: String,
  pub integrity: String,
  pub dependencies: Vec<NpmPackageDependencyLockfileInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpmPackageDependencyLockfileInfo {
  pub name: String,
  pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct NpmPackageInfo {
  pub integrity: String,
  // todo(dsherret): we should skip serializing this in a future lockfile version
  pub dependencies: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash)]
pub struct JsrPackageInfo {
  pub integrity: String,
  /// List of package requirements found in the dependency.
  ///
  /// This is used to tell when a package can be removed from the lockfile.
  #[serde(skip_serializing_if = "BTreeSet::is_empty")]
  #[serde(default)]
  pub dependencies: BTreeSet<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, Hash)]
pub struct PackagesContent {
  /// Mapping between requests for deno specifiers and resolved packages, eg.
  /// {
  ///   "jsr:@foo/bar@^2.1": "jsr:@foo/bar@2.1.3",
  ///   "npm:@ts-morph/common@^11": "npm:@ts-morph/common@11.0.0",
  /// }
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  #[serde(default)]
  pub specifiers: BTreeMap<String, String>,

  /// Mapping between resolved jsr specifiers and their associated info, eg.
  /// {
  ///   "@oak/oak@12.6.3": {
  ///     "dependencies": [
  ///       "jsr:@std/bytes@0.210",
  ///       // ...etc...
  ///       "npm:path-to-regexpr@^6.2"
  ///     ]
  ///   }
  /// }
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  #[serde(default)]
  pub jsr: BTreeMap<String, JsrPackageInfo>,

  /// Mapping between resolved npm specifiers and their associated info, eg.
  /// {
  ///   "chalk@5.0.0": {
  ///     "integrity": "sha512-...",
  ///     "dependencies": {
  ///       "ansi-styles": "ansi-styles@4.1.0",
  ///     }
  ///   }
  /// }
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  #[serde(default)]
  pub npm: BTreeMap<String, NpmPackageInfo>,
}

impl PackagesContent {
  fn is_empty(&self) -> bool {
    self.specifiers.is_empty() && self.npm.is_empty() && self.jsr.is_empty()
  }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, Hash)]
#[serde(rename_all = "camelCase")]
struct LockfilePackageJsonContent {
  #[serde(default)]
  #[serde(skip_serializing_if = "BTreeSet::is_empty")]
  dependencies: BTreeSet<String>,
}

impl LockfilePackageJsonContent {
  pub fn is_empty(&self) -> bool {
    self.dependencies.is_empty()
  }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, Hash)]
#[serde(rename_all = "camelCase")]
struct WorkspaceMemberConfigContent {
  #[serde(skip_serializing_if = "BTreeSet::is_empty")]
  #[serde(default)]
  dependencies: BTreeSet<String>,
  #[serde(skip_serializing_if = "LockfilePackageJsonContent::is_empty")]
  #[serde(default)]
  package_json: LockfilePackageJsonContent,
}

impl WorkspaceMemberConfigContent {
  pub fn is_empty(&self) -> bool {
    self.dependencies.is_empty() && self.package_json.is_empty()
  }

  pub fn dep_reqs(&self) -> impl Iterator<Item = &String> {
    self
      .package_json
      .dependencies
      .iter()
      .chain(self.dependencies.iter())
  }
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
  pub fn is_empty(&self) -> bool {
    self.root.is_empty() && self.members.is_empty()
  }

  fn get_all_dep_reqs(&self) -> impl Iterator<Item = &String> {
    self
      .root
      .dep_reqs()
      .chain(self.members.values().flat_map(|m| m.dep_reqs()))
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash)]
#[serde(rename_all = "camelCase")]
pub struct LockfileContent {
  version: String,
  // order these based on auditability
  #[serde(skip_serializing_if = "PackagesContent::is_empty")]
  #[serde(default)]
  pub packages: PackagesContent,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  #[serde(default)]
  pub redirects: BTreeMap<String, String>,
  // todo(dsherret): in the next lockfile version we should skip
  // serializing this when it's empty
  /// Mapping between URLs and their checksums for "http:" and "https:" deps
  #[serde(default)]
  remote: BTreeMap<String, String>,
  #[serde(skip_serializing_if = "WorkspaceConfigContent::is_empty")]
  #[serde(default)]
  workspace: WorkspaceConfigContent,
}

impl LockfileContent {
  fn empty() -> Self {
    Self {
      version: "3".to_string(),
      packages: Default::default(),
      redirects: Default::default(),
      remote: BTreeMap::new(),
      workspace: Default::default(),
    }
  }

  pub fn is_empty(&self) -> bool {
    self.packages.is_empty()
      && self.redirects.is_empty()
      && self.remote.is_empty()
      && self.workspace.is_empty()
  }
}

#[derive(Debug, Clone, Hash)]
pub struct Lockfile {
  pub overwrite: bool,
  pub has_content_changed: bool,
  pub content: LockfileContent,
  pub filename: PathBuf,
}

impl Lockfile {
  pub fn new_empty(filename: PathBuf, overwrite: bool) -> Lockfile {
    Lockfile {
      overwrite,
      has_content_changed: false,
      content: LockfileContent::empty(),
      filename,
    }
  }

  /// Create a new [`Lockfile`] instance from given filename and its content.
  pub fn with_lockfile_content(
    filename: PathBuf,
    content: &str,
    overwrite: bool,
  ) -> Result<Lockfile, Error> {
    // Writing a lock file always uses the new format.
    if overwrite {
      return Ok(Lockfile::new_empty(filename, overwrite));
    }

    if content.trim().is_empty() {
      return Err(Error::ReadError("Lockfile was empty.".to_string()));
    }

    let value: serde_json::Map<String, serde_json::Value> =
      serde_json::from_str(content).map_err(|err| {
        Error::ParseError(filename.display().to_string(), err)
      })?;
    let version = value.get("version").and_then(|v| v.as_str());
    let value = match version {
      Some("3") => value,
      Some("2") => transforms::transform2_to_3(value),
      None => transforms::transform2_to_3(transforms::transform1_to_2(value)),
      Some(version) => {
        return Err(Error::UnsupportedVersion(version.to_string()));
      }
    };
    let content = serde_json::from_value::<LockfileContent>(value.into())
      .map_err(|err| Error::ParseError(filename.display().to_string(), err))?;

    Ok(Lockfile {
      overwrite,
      has_content_changed: false,
      content,
      filename,
    })
  }

  pub fn as_json_string(&self) -> String {
    let mut json_string = serde_json::to_string_pretty(&self.content).unwrap();
    json_string.push('\n'); // trailing newline in file
    json_string
  }

  pub fn set_workspace_config(
    &mut self,
    mut options: SetWorkspaceConfigOptions,
  ) {
    fn update_workspace_member(
      has_content_changed: &mut bool,
      removed_deps: &mut HashSet<String>,
      current: &mut WorkspaceMemberConfigContent,
      new: WorkspaceMemberConfig,
    ) {
      if new.dependencies != current.dependencies {
        let old_deps =
          std::mem::replace(&mut current.dependencies, new.dependencies);

        removed_deps.extend(old_deps);

        *has_content_changed = true;
      }

      if new.package_json_deps != current.package_json.dependencies {
        // update self.content.package_json
        let old_package_json_deps = std::mem::replace(
          &mut current.package_json.dependencies,
          new.package_json_deps,
        );

        removed_deps.extend(old_package_json_deps);

        *has_content_changed = true;
      }
    }

    // if specified, don't modify the package.json dependencies
    if options.no_npm || options.no_config {
      if options.config.root.package_json_deps.is_empty() {
        options.config.root.package_json_deps = self
          .content
          .workspace
          .root
          .package_json
          .dependencies
          .clone();
      }
      for (key, value) in options.config.members.iter_mut() {
        if value.package_json_deps.is_empty() {
          value.package_json_deps = self
            .content
            .workspace
            .members
            .get(key)
            .map(|m| m.package_json.dependencies.clone())
            .unwrap_or_default();
        }
      }
    }
    if options.no_config {
      if options.config.root.dependencies.is_empty() {
        options.config.root.dependencies =
          self.content.workspace.root.dependencies.clone();
      }
      for (key, value) in options.config.members.iter_mut() {
        if value.dependencies.is_empty() {
          value.dependencies = self
            .content
            .workspace
            .members
            .get(key)
            .map(|m| m.dependencies.clone())
            .unwrap_or_default();
        }
      }
      for (key, value) in self.content.workspace.members.iter() {
        if options.config.members.get(key).is_none() {
          options.config.members.insert(
            key.clone(),
            WorkspaceMemberConfig {
              dependencies: value.dependencies.clone(),
              package_json_deps: value.package_json.dependencies.clone(),
            },
          );
        }
      }
    }

    // If the lockfile is empty, it's most likely not created yet and so
    // we don't want this information being added to the lockfile to cause
    // a lockfile to be created. If this is the case, revert the lockfile back
    // to !self.has_content_changed after populating it with this information
    let allow_content_changed =
      self.has_content_changed || !self.content.is_empty();
    let old_deps = self
      .content
      .workspace
      .get_all_dep_reqs()
      .map(|s| s.to_string())
      .collect::<HashSet<_>>();
    let mut removed_deps = HashSet::new();

    // set the root
    update_workspace_member(
      &mut self.has_content_changed,
      &mut removed_deps,
      &mut self.content.workspace.root,
      options.config.root,
    );

    // now go through the workspaces
    let mut unhandled_members = self
      .content
      .workspace
      .members
      .keys()
      .cloned()
      .collect::<HashSet<_>>();
    for (member_name, new_member) in options.config.members {
      unhandled_members.remove(&member_name);
      let current_member = self
        .content
        .workspace
        .members
        .entry(member_name)
        .or_default();
      update_workspace_member(
        &mut self.has_content_changed,
        &mut removed_deps,
        current_member,
        new_member,
      );
    }

    for member in unhandled_members {
      if let Some(member) = self.content.workspace.members.remove(&member) {
        removed_deps.extend(member.dep_reqs().cloned());
        self.has_content_changed = true;
      }
    }

    // update the removed deps to keep what's still found in the workspace
    for dep in self.content.workspace.get_all_dep_reqs() {
      removed_deps.remove(dep);
    }

    if !removed_deps.is_empty() {
      let packages = std::mem::take(&mut self.content.packages);
      let remotes = std::mem::take(&mut self.content.remote);

      // create the graph
      let mut graph = LockfilePackageGraph::from_lockfile(
        packages,
        remotes,
        old_deps.iter().map(|dep| dep.as_str()),
      );

      // remove the packages
      graph.remove_root_packages(removed_deps.into_iter());

      // now populate the graph back into the packages
      graph.populate_packages(
        &mut self.content.packages,
        &mut self.content.remote,
      );
    }

    if !allow_content_changed {
      // revert it back so this change doesn't by itself cause
      // a lockfile to be created.
      self.has_content_changed = false;
    }
  }

  /// Gets the bytes that should be written to the disk.
  ///
  /// Ideally when the caller should use an "atomic write"
  /// when writing this—write to a temporary file beside the
  /// lockfile, then rename to overwrite. This will make the
  /// lockfile more resilient when multiple processes are
  /// writing to it.
  pub fn resolve_write_bytes(&self) -> Option<Vec<u8>> {
    if !self.has_content_changed && !self.overwrite {
      return None;
    }

    Some(self.as_json_string().into_bytes())
  }

  pub fn remote(&self) -> &BTreeMap<String, String> {
    &self.content.remote
  }

  /// Inserts a remote specifier into the lockfile replacing the existing package if it exists.
  ///
  /// WARNING: It is up to the caller to ensure checksums of remote modules are
  /// valid before it is inserted here.
  pub fn insert_remote(&mut self, specifier: String, hash: String) {
    let entry = self.content.remote.entry(specifier);
    match entry {
      Entry::Vacant(entry) => {
        entry.insert(hash);
        self.has_content_changed = true;
      }
      Entry::Occupied(mut entry) => {
        if entry.get() != &hash {
          entry.insert(hash);
          self.has_content_changed = true;
        }
      }
    }
  }

  /// Inserts an npm package into the lockfile replacing the existing package if it exists.
  ///
  /// WARNING: It is up to the caller to ensure checksums of packages are
  /// valid before it is inserted here.
  pub fn insert_npm_package(&mut self, package_info: NpmPackageLockfileInfo) {
    let dependencies = package_info
      .dependencies
      .into_iter()
      .map(|dep| (dep.name, dep.id))
      .collect::<BTreeMap<String, String>>();

    let entry = self.content.packages.npm.entry(package_info.serialized_id);
    let package_info = NpmPackageInfo {
      integrity: package_info.integrity,
      dependencies,
    };
    match entry {
      Entry::Vacant(entry) => {
        entry.insert(package_info);
        self.has_content_changed = true;
      }
      Entry::Occupied(mut entry) => {
        if *entry.get() != package_info {
          entry.insert(package_info);
          self.has_content_changed = true;
        }
      }
    }
  }

  /// Inserts a package specifier into the lockfile.
  pub fn insert_package_specifier(
    &mut self,
    serialized_package_req: String,
    serialized_package_id: String,
  ) {
    let entry = self
      .content
      .packages
      .specifiers
      .entry(serialized_package_req);
    match entry {
      Entry::Vacant(entry) => {
        entry.insert(serialized_package_id);
        self.has_content_changed = true;
      }
      Entry::Occupied(mut entry) => {
        if *entry.get() != serialized_package_id {
          entry.insert(serialized_package_id);
          self.has_content_changed = true;
        }
      }
    }
  }

  /// Inserts a JSR package into the lockfile replacing the existing package's integrity
  /// if they differ.
  ///
  /// WARNING: It is up to the caller to ensure checksums of packages are
  /// valid before it is inserted here.
  pub fn insert_package(&mut self, name: String, integrity: String) {
    let entry = self.content.packages.jsr.entry(name);
    match entry {
      Entry::Vacant(entry) => {
        entry.insert(JsrPackageInfo {
          integrity,
          dependencies: Default::default(),
        });
        self.has_content_changed = true;
      }
      Entry::Occupied(mut entry) => {
        if *entry.get().integrity != integrity {
          entry.get_mut().integrity = integrity;
          self.has_content_changed = true;
        }
      }
    }
  }

  /// Adds package dependencies of a JSR package. This is only used to track
  /// when packages can be removed from the lockfile.
  pub fn add_package_deps(
    &mut self,
    name: &str,
    deps: impl Iterator<Item = String>,
  ) {
    if let Some(pkg) = self.content.packages.jsr.get_mut(name) {
      let start_count = pkg.dependencies.len();
      pkg.dependencies.extend(deps);
      let end_count = pkg.dependencies.len();
      if start_count != end_count {
        self.has_content_changed = true;
      }
    }
  }

  pub fn insert_redirect(&mut self, from: String, to: String) {
    if from.starts_with("jsr:") {
      return;
    }

    let entry = self.content.redirects.entry(from);
    match entry {
      Entry::Vacant(entry) => {
        entry.insert(to);
        self.has_content_changed = true;
      }
      Entry::Occupied(mut entry) => {
        if *entry.get() != to {
          entry.insert(to);
          self.has_content_changed = true;
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use pretty_assertions::assert_eq;

  const LOCKFILE_JSON: &str = r#"
{
  "version": "3",
  "packages": {
    "specifiers": {},
    "npm": {
      "nanoid@3.3.4": {
        "integrity": "sha512-MqBkQh/OHTS2egovRtLk45wEyNXwF+cokD+1YPf9u5VfJiRdAiRwB2froX5Co9Rh20xs4siNPm8naNotSD6RBw==",
        "dependencies": {}
      },
      "picocolors@1.0.0": {
        "integrity": "sha512-foobar",
        "dependencies": {}
      }
    }
  },
  "remote": {
    "https://deno.land/std@0.71.0/textproto/mod.ts": "3118d7a42c03c242c5a49c2ad91c8396110e14acca1324e7aaefd31a999b71a4",
    "https://deno.land/std@0.71.0/async/delay.ts": "35957d585a6e3dd87706858fb1d6b551cb278271b03f52c5a2cb70e65e00c26a"
  }
}"#;

  fn setup(overwrite: bool) -> Result<Lockfile, Error> {
    let file_path =
      std::env::current_dir().unwrap().join("valid_lockfile.json");
    Lockfile::with_lockfile_content(file_path, LOCKFILE_JSON, overwrite)
  }

  #[test]
  fn future_version_unsupported() {
    let file_path = PathBuf::from("lockfile.json");
    assert_eq!(
      Lockfile::with_lockfile_content(
        file_path,
        "{ \"version\": \"2000\" }",
        false
      )
      .err()
      .unwrap().to_string(),
      "Unsupported lockfile version '2000'. Try upgrading Deno or recreating the lockfile.".to_string()
    );
  }

  #[test]
  fn new_valid_lockfile() {
    let lockfile = setup(false).unwrap();

    let remote = lockfile.content.remote;
    let keys: Vec<String> = remote.keys().cloned().collect();
    let expected_keys = vec![
      String::from("https://deno.land/std@0.71.0/async/delay.ts"),
      String::from("https://deno.land/std@0.71.0/textproto/mod.ts"),
    ];

    assert_eq!(keys.len(), 2);
    assert_eq!(keys, expected_keys);
  }

  #[test]
  fn with_lockfile_content_for_valid_lockfile() {
    let file_path = PathBuf::from("/foo");
    let result =
      Lockfile::with_lockfile_content(file_path, LOCKFILE_JSON, false).unwrap();

    let remote = result.content.remote;
    let keys: Vec<String> = remote.keys().cloned().collect();
    let expected_keys = vec![
      String::from("https://deno.land/std@0.71.0/async/delay.ts"),
      String::from("https://deno.land/std@0.71.0/textproto/mod.ts"),
    ];

    assert_eq!(keys.len(), 2);
    assert_eq!(keys, expected_keys);
  }

  #[test]
  fn new_lockfile_from_file_and_insert() {
    let mut lockfile = setup(false).unwrap();

    lockfile.insert_remote(
      "https://deno.land/std@0.71.0/io/util.ts".to_string(),
      "checksum-1".to_string(),
    );

    let remote = lockfile.content.remote;
    let keys: Vec<String> = remote.keys().cloned().collect();
    let expected_keys = vec![
      String::from("https://deno.land/std@0.71.0/async/delay.ts"),
      String::from("https://deno.land/std@0.71.0/io/util.ts"),
      String::from("https://deno.land/std@0.71.0/textproto/mod.ts"),
    ];
    assert_eq!(keys.len(), 3);
    assert_eq!(keys, expected_keys);
  }

  #[test]
  fn new_lockfile_and_write() {
    let mut lockfile = setup(true).unwrap();

    // true since overwrite was true
    assert!(lockfile.resolve_write_bytes().is_some());

    lockfile.insert_remote(
      "https://deno.land/std@0.71.0/textproto/mod.ts".to_string(),
      "checksum-1".to_string(),
    );
    lockfile.insert_remote(
      "https://deno.land/std@0.71.0/io/util.ts".to_string(),
      "checksum-2".to_string(),
    );
    lockfile.insert_remote(
      "https://deno.land/std@0.71.0/async/delay.ts".to_string(),
      "checksum-3".to_string(),
    );

    let bytes = lockfile.resolve_write_bytes().unwrap();
    let contents_json =
      serde_json::from_slice::<serde_json::Value>(&bytes).unwrap();
    let object = contents_json["remote"].as_object().unwrap();

    assert_eq!(
      object
        .get("https://deno.land/std@0.71.0/textproto/mod.ts")
        .and_then(|v| v.as_str()),
      Some("checksum-1")
    );

    // confirm that keys are sorted alphabetically
    let mut keys = object.keys().map(|k| k.as_str());
    assert_eq!(
      keys.next(),
      Some("https://deno.land/std@0.71.0/async/delay.ts")
    );
    assert_eq!(keys.next(), Some("https://deno.land/std@0.71.0/io/util.ts"));
    assert_eq!(
      keys.next(),
      Some("https://deno.land/std@0.71.0/textproto/mod.ts")
    );
    assert!(keys.next().is_none());
  }

  #[test]
  fn check_or_insert_lockfile() {
    let mut lockfile = setup(false).unwrap();

    // none since overwrite was false and there's no changes
    assert!(lockfile.resolve_write_bytes().is_none());

    lockfile.insert_remote(
      "https://deno.land/std@0.71.0/textproto/mod.ts".to_string(),
      "checksum-1".to_string(),
    );
    assert!(lockfile.has_content_changed);

    lockfile.has_content_changed = false;
    lockfile.insert_remote(
      "https://deno.land/std@0.71.0/textproto/mod.ts".to_string(),
      "checksum-1".to_string(),
    );
    assert!(!lockfile.has_content_changed);

    lockfile.insert_remote(
      "https://deno.land/std@0.71.0/textproto/mod.ts".to_string(),
      "checksum-new".to_string(),
    );
    assert!(lockfile.has_content_changed);
    lockfile.has_content_changed = false;

    // Not present in lockfile yet, should be inserted and check passed.
    lockfile.insert_remote(
      "https://deno.land/std@0.71.0/http/file_server.ts".to_string(),
      "checksum-1".to_string(),
    );
    assert!(lockfile.has_content_changed);

    // true since there were changes
    assert!(lockfile.resolve_write_bytes().is_some());
  }

  #[test]
  fn check_or_insert_lockfile_npm() {
    let mut lockfile = setup(false).unwrap();

    // already in lockfile
    let npm_package = NpmPackageLockfileInfo {
      serialized_id: "nanoid@3.3.4".to_string(),
      integrity: "sha512-MqBkQh/OHTS2egovRtLk45wEyNXwF+cokD+1YPf9u5VfJiRdAiRwB2froX5Co9Rh20xs4siNPm8naNotSD6RBw==".to_string(),
      dependencies: vec![],
    };
    lockfile.insert_npm_package(npm_package);
    assert!(!lockfile.has_content_changed);

    // insert package that exists already, but has slightly different properties
    let npm_package = NpmPackageLockfileInfo {
      serialized_id: "picocolors@1.0.0".to_string(),
      integrity: "sha512-1fygroTLlHu66zi26VoTDv8yRgm0Fccecssto+MhsZ0D/DGW2sm8E8AjW7NU5VVTRt5GxbeZ5qBuJr+HyLYkjQ==".to_string(),
      dependencies: vec![],
    };
    lockfile.insert_npm_package(npm_package);
    assert!(lockfile.has_content_changed);

    lockfile.has_content_changed = false;
    let npm_package = NpmPackageLockfileInfo {
      serialized_id: "source-map-js@1.0.2".to_string(),
      integrity: "sha512-R0XvVJ9WusLiqTCEiGCmICCMplcCkIwwR11mOSD9CR5u+IXYdiseeEuXCVAjS54zqwkLcPNnmU4OeJ6tUrWhDw==".to_string(),
      dependencies: vec![],
    };
    // Not present in lockfile yet, should be inserted
    lockfile.insert_npm_package(npm_package.clone());
    assert!(lockfile.has_content_changed);
    lockfile.has_content_changed = false;

    // this one should not say the lockfile has changed because it's the same
    lockfile.insert_npm_package(npm_package);
    assert!(!lockfile.has_content_changed);

    let npm_package = NpmPackageLockfileInfo {
      serialized_id: "source-map-js@1.0.2".to_string(),
      integrity: "sha512-foobar".to_string(),
      dependencies: vec![],
    };
    // Now present in lockfile, should be changed due to different integrity
    lockfile.insert_npm_package(npm_package);
    assert!(lockfile.has_content_changed);
  }

  #[test]
  fn lockfile_with_redirects() {
    let mut lockfile = Lockfile::with_lockfile_content(
      PathBuf::from("/foo/deno.lock"),
      r#"{
  "version": "3",
  "redirects": {
    "https://deno.land/x/std/mod.ts": "https://deno.land/std@0.190.0/mod.ts"
  },
  "remote": {}
}"#,
      false,
    )
    .unwrap();
    lockfile.content.redirects.insert(
      "https://deno.land/x/other/mod.ts".to_string(),
      "https://deno.land/x/other@0.1.0/mod.ts".to_string(),
    );
    assert_eq!(
      lockfile.as_json_string(),
      r#"{
  "version": "3",
  "redirects": {
    "https://deno.land/x/other/mod.ts": "https://deno.land/x/other@0.1.0/mod.ts",
    "https://deno.land/x/std/mod.ts": "https://deno.land/std@0.190.0/mod.ts"
  },
  "remote": {}
}
"#,
    );
  }

  #[test]
  fn test_insert_redirect() {
    let mut lockfile = Lockfile::with_lockfile_content(
      PathBuf::from("/foo/deno.lock"),
      r#"{
  "version": "3",
  "redirects": {
    "https://deno.land/x/std/mod.ts": "https://deno.land/std@0.190.0/mod.ts"
  },
  "remote": {}
}"#,
      false,
    )
    .unwrap();
    lockfile.insert_redirect(
      "https://deno.land/x/std/mod.ts".to_string(),
      "https://deno.land/std@0.190.0/mod.ts".to_string(),
    );
    assert!(!lockfile.has_content_changed);
    lockfile.insert_redirect(
      "https://deno.land/x/std/mod.ts".to_string(),
      "https://deno.land/std@0.190.1/mod.ts".to_string(),
    );
    assert!(lockfile.has_content_changed);
    lockfile.insert_redirect(
      "https://deno.land/x/std/other.ts".to_string(),
      "https://deno.land/std@0.190.1/other.ts".to_string(),
    );
    assert_eq!(
      lockfile.as_json_string(),
      r#"{
  "version": "3",
  "redirects": {
    "https://deno.land/x/std/mod.ts": "https://deno.land/std@0.190.1/mod.ts",
    "https://deno.land/x/std/other.ts": "https://deno.land/std@0.190.1/other.ts"
  },
  "remote": {}
}
"#,
    );
  }

  #[test]
  fn test_insert_jsr() {
    let mut lockfile = Lockfile::with_lockfile_content(
      PathBuf::from("/foo/deno.lock"),
      r#"{
  "version": "3",
  "packages": {
    "specifiers": {
      "jsr:path": "jsr:@std/path@0.75.0"
    }
  },
  "remote": {}
}"#,
      false,
    )
    .unwrap();
    lockfile.insert_package_specifier(
      "jsr:path".to_string(),
      "jsr:@std/path@0.75.0".to_string(),
    );
    assert!(!lockfile.has_content_changed);
    lockfile.insert_package_specifier(
      "jsr:path".to_string(),
      "jsr:@std/path@0.75.1".to_string(),
    );
    assert!(lockfile.has_content_changed);
    lockfile.insert_package_specifier(
      "jsr:@foo/bar@^2".to_string(),
      "jsr:@foo/bar@2.1.2".to_string(),
    );
    assert_eq!(
      lockfile.as_json_string(),
      r#"{
  "version": "3",
  "packages": {
    "specifiers": {
      "jsr:@foo/bar@^2": "jsr:@foo/bar@2.1.2",
      "jsr:path": "jsr:@std/path@0.75.1"
    }
  },
  "remote": {}
}
"#,
    );
  }

  #[test]
  fn read_version_1() {
    let content: &str = r#"{
      "https://deno.land/std@0.71.0/textproto/mod.ts": "3118d7a42c03c242c5a49c2ad91c8396110e14acca1324e7aaefd31a999b71a4",
      "https://deno.land/std@0.71.0/async/delay.ts": "35957d585a6e3dd87706858fb1d6b551cb278271b03f52c5a2cb70e65e00c26a"
    }"#;
    let file_path = PathBuf::from("lockfile.json");
    let lockfile =
      Lockfile::with_lockfile_content(file_path, content, false).unwrap();
    assert_eq!(lockfile.content.version, "3");
    assert_eq!(lockfile.content.remote.len(), 2);
  }

  #[test]
  fn read_version_2() {
    let content: &str = r#"{
      "version": "2",
      "remote": {
        "https://deno.land/std@0.71.0/textproto/mod.ts": "3118d7a42c03c242c5a49c2ad91c8396110e14acca1324e7aaefd31a999b71a4",
        "https://deno.land/std@0.71.0/async/delay.ts": "35957d585a6e3dd87706858fb1d6b551cb278271b03f52c5a2cb70e65e00c26a"
      },
      "npm": {
        "specifiers": {
          "nanoid": "nanoid@3.3.4"
        },
        "packages": {
          "nanoid@3.3.4": {
            "integrity": "sha512-MqBkQh/OHTS2egovRtLk45wEyNXwF+cokD+1YPf9u5VfJiRdAiRwB2froX5Co9Rh20xs4siNPm8naNotSD6RBw==",
            "dependencies": {}
          },
          "picocolors@1.0.0": {
            "integrity": "sha512-foobar",
            "dependencies": {}
          }
        }
      }
    }"#;
    let file_path = PathBuf::from("lockfile.json");
    let lockfile =
      Lockfile::with_lockfile_content(file_path, content, false).unwrap();
    assert_eq!(lockfile.content.version, "3");
    assert_eq!(lockfile.content.packages.npm.len(), 2);
    assert_eq!(
      lockfile.content.packages.specifiers,
      BTreeMap::from([(
        "npm:nanoid".to_string(),
        "npm:nanoid@3.3.4".to_string()
      )])
    );
    assert_eq!(lockfile.content.remote.len(), 2);
  }

  #[test]
  fn insert_package_deps_changes_empty_insert() {
    let content: &str = r#"{
      "version": "2",
      "remote": {}
    }"#;
    let file_path = PathBuf::from("lockfile.json");
    let mut lockfile =
      Lockfile::with_lockfile_content(file_path, content, false).unwrap();

    assert!(!lockfile.has_content_changed);
    lockfile.insert_package("dep".to_string(), "integrity".to_string());
    // has changed even though it was empty
    assert!(lockfile.has_content_changed);

    // now try inserting the same package
    lockfile.has_content_changed = false;
    lockfile.insert_package("dep".to_string(), "integrity".to_string());
    assert!(!lockfile.has_content_changed);

    // now with new deps
    lockfile.add_package_deps("dep", vec!["dep2".to_string()].into_iter());
    assert!(lockfile.has_content_changed);
  }

  #[test]
  fn empty_lockfile_nicer_error() {
    let content: &str = r#"  "#;
    let file_path = PathBuf::from("lockfile.json");
    let err = Lockfile::with_lockfile_content(file_path, content, false)
      .err()
      .unwrap();
    assert_eq!(
      err.to_string(),
      "Unable to read lockfile. Lockfile was empty."
    );
  }
}
