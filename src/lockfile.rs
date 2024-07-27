// Copyright 2018-2024 the Deno authors. MIT license.

mod lockfile_content;
mod workspace_config;

pub use lockfile_content::JsrPackageInfo;
pub use lockfile_content::LockfileContent;
pub use lockfile_content::NpmPackageDependencyLockfileInfo;
pub use lockfile_content::NpmPackageInfo;
pub use lockfile_content::NpmPackageLockfileInfo;
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::transforms::transform1_to_2;
use crate::transforms::transform2_to_3;
use crate::transforms::transform3_to_4;
use crate::LockfileError;
use crate::LockfileErrorReason;
pub use workspace_config::*;

use crate::graphs::LockfilePackageGraph;

#[derive(Debug, Clone, Hash)]
pub struct Lockfile {
  /// If this flag is set, the current content of the lockfile is ignored and a new lockfile is generated.
  ///
  /// If it is unset, the lockfile will only be changed, if the content changed.
  overwrite: bool,
  /// Automatically set to true, if the content of the lockfile has changed.
  ///
  /// Once this flag is set to true, it will never be reset to false, except through [Lockfile::resolve_write_bytes]
  has_content_changed: bool,
  /// Current content of the lockfile
  content: LockfileContent,
  /// Path of the lockfile
  filename: PathBuf,
  /// Original content of the lockfile
  ///
  /// We need to store this, so that [Lockfile::to_json] can return the exact original content, if there were no changes
  original_content: Option<String>,
}

/// Represents a `deno.lock` lockfile
impl Lockfile {
  /// Create a new empty [`Lockfile`] instance
  pub fn new(filename: PathBuf, overwrite: bool) -> Lockfile {
    Lockfile {
      overwrite,
      has_content_changed: false,
      content: LockfileContent::new(),
      filename,
      original_content: Option::Some(String::new()),
    }
  }

  /// Create a new [`Lockfile`] instance with the supplied content
  pub fn with_lockfile_content(
    filename: PathBuf,
    file_content: &str,
    overwrite: bool,
  ) -> Result<Lockfile, LockfileError> {
    fn load_content(
      content: &str,
    ) -> Result<LockfileContent, LockfileErrorReason> {
      let value: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(content)
          .map_err(LockfileErrorReason::ParseError)?;
      let version = value.get("version").and_then(|v| v.as_str());
      let value = match version {
        Some("4") => value,
        Some("3") => transform3_to_4(value)?,
        Some("2") => transform3_to_4(transform2_to_3(value))?,
        None => transform3_to_4(transform2_to_3(transform1_to_2(value)))?,
        Some(version) => {
          return Err(LockfileErrorReason::UnsupportedVersion {
            version: version.to_string(),
          });
        }
      };
      let content = LockfileContent::from_json(value.into())
        .map_err(LockfileErrorReason::DeserializationError)?;

      Ok(content)
    }

    // Writing a lock file always uses the new format.
    if overwrite {
      return Ok(Lockfile::new(filename, overwrite));
    }

    if file_content.trim().is_empty() {
      return Err(LockfileError {
        filename: filename.display().to_string(),
        reason: LockfileErrorReason::Empty,
      });
    }

    let content =
      load_content(file_content).map_err(|reason| LockfileError {
        filename: filename.display().to_string(),
        reason,
      })?;
    Ok(Lockfile {
      overwrite,
      has_content_changed: false,
      content,
      filename,
      original_content: Some(file_content.into()),
    })
  }

  /// Get the lockfile contents as a formatted JSON string
  ///
  /// If no changes were done, this will return the exact lockfile content that was used to create this lockfile.
  ///
  /// If the lockfile was changed, it will be returned as an upgraded v4 lockfile
  pub fn to_json(&self) -> String {
    if let Some(original_content) = &self.original_content {
      if !self.has_content_changed && !self.overwrite {
        return original_content.clone();
      }
    }

    if self.content.version != "4" {
      panic!("Should never happen; for now only v4 lockfiles can be printed")
    }
    self.content.to_json()
  }

  /// Gets the bytes that should be written to the disk.
  ///
  /// Ideally when the caller should use an "atomic write" when writing this—write to a temporary file beside the lockfile, then rename to overwrite. This will make the lockfile more resilient when multiple processes are writing to it.
  ///
  /// If you dont write the bytes received by this function to the lockfile, it will result in undefined behaviour
  // TODO: Resetting `has_content_change` probably has some funny side effects; investigate
  pub fn resolve_write_bytes(&mut self) -> Option<Vec<u8>> {
    if !self.has_content_changed && !self.overwrite {
      return None;
    }

    // This weird order is neccessary, because to_json will return the original_content, if there
    let json_string = self.to_json();
    self.has_content_changed = false;
    self.original_content = Some(json_string.clone());
    Some(json_string.into_bytes())
  }

  /// Get the lockfile content
  pub fn content(&self) -> &LockfileContent {
    &self.content
  }

  /// Get the filename of the lockfile
  pub fn filename(&self) -> &PathBuf {
    &self.filename
  }

  /// Returns true, if we performed operations that modified the lockfile
  ///
  /// Will be reset to false after the content was read with [Lockfile::resolve_write_bytes]
  pub fn has_content_changed(&self) -> bool {
    self.has_content_changed
  }

  /// Check if this lockfile was created with overwrite
  // TODO: Document what exactly overwrite does
  pub fn overwrite(&self) -> bool {
    self.overwrite
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

  /// Removes a remote from the lockfile
  ///
  /// Returns the hash of the removed remote, if something was removed
  pub fn remove_remote(&mut self, from: &str) -> Option<String> {
    let removed_value = self.content.remote.remove(from);
    if removed_value.is_some() {
      self.has_content_changed = true;
    }
    removed_value
  }

  /// Adds a redirect to the lockfile
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

  /// Removes a redirect from the lockfile
  ///
  /// Returns the target of the removed redirect.
  pub fn remove_redirect(&mut self, from: &str) -> Option<String> {
    let removed_value = self.content.redirects.remove(from);
    if removed_value.is_some() {
      self.has_content_changed = true;
    }
    removed_value
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

    let entry = self.content.npm.entry(package_info.serialized_id);
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
    let entry = self.content.specifiers.entry(serialized_package_req);
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
  pub fn insert_jsr_package(&mut self, name: String, integrity: String) {
    let entry = self.content.jsr.entry(name);
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
    if let Some(pkg) = self.content.jsr.get_mut(name) {
      let start_count = pkg.dependencies.len();
      pkg.dependencies.extend(deps);
      let end_count = pkg.dependencies.len();
      if start_count != end_count {
        self.has_content_changed = true;
      }
    }
  }

  /// Set the workspace config
  pub fn set_workspace_config(&mut self, options: SetWorkspaceConfigOptions) {
    let was_empty_before = self.content.is_empty();
    let old_workspace_config = self.content.workspace.clone();

    // Update the workspace
    let config = WorkspaceConfig::new(options, &self.content.workspace);
    self.content.workspace.update(config);

    // We dont need to do the rest, if we changed nothing
    if old_workspace_config == self.content.workspace {
      return;
    }

    // If the lockfile is empty, it's most likely not created yet and so
    // we don't want workspace configuration being added to the lockfile to cause
    // a lockfile to be created.
    // So we only set has_content_changed if it wasnt empty before
    if !was_empty_before {
      // revert it back so this change doesn't by itself cause
      // a lockfile to be created.
      self.has_content_changed = true;
    }

    let old_deps: BTreeSet<&String> =
      old_workspace_config.get_all_dep_reqs().collect();
    let new_deps: BTreeSet<&String> =
      self.content.workspace.get_all_dep_reqs().collect();
    let removed_deps: BTreeSet<&String> =
      old_deps.difference(&new_deps).copied().collect();

    if removed_deps.is_empty() {
      return;
    }

    // Remove removed dependencies from packages and remote
    let npm = std::mem::take(&mut self.content.npm);
    let jsr = std::mem::take(&mut self.content.jsr);
    let specifiers = std::mem::take(&mut self.content.specifiers);
    let mut graph = LockfilePackageGraph::from_lockfile(
      npm,
      jsr,
      specifiers,
      old_deps.iter().map(|dep| dep.as_str()),
    );
    graph.remove_root_packages(removed_deps.into_iter());
    graph.populate_packages(
      &mut self.content.npm,
      &mut self.content.jsr,
      &mut self.content.specifiers,
    );
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

  fn setup(overwrite: bool) -> Result<Lockfile, LockfileError> {
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
      "Unsupported lockfile version '2000'. Try upgrading Deno or recreating the lockfile at 'lockfile.json'.".to_string()
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
    // assert!(lockfile.resolve_write_bytes().is_some());

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
    // Setup lockfile
    lockfile.insert_remote(
      "https://deno.land/std@0.71.0/textproto/mod.ts".to_string(),
      "checksum-1".to_string(),
    );
    // By reading the bytes we reset the changed state
    assert!(lockfile.resolve_write_bytes().is_some());
    // Verify that the lockfile has no unwritten changes
    assert!(lockfile.resolve_write_bytes().is_none());

    // Not a change, should not cause changes
    lockfile.insert_remote(
      "https://deno.land/std@0.71.0/textproto/mod.ts".to_string(),
      "checksum-1".to_string(),
    );
    assert!(lockfile.resolve_write_bytes().is_none());

    // This is a change, it should cause a write
    lockfile.insert_remote(
      "https://deno.land/std@0.71.0/textproto/mod.ts".to_string(),
      "checksum-new".to_string(),
    );
    assert!(lockfile.resolve_write_bytes().is_some());

    // Not present in lockfile yet, should be inserted and check passed.
    lockfile.insert_remote(
      "https://deno.land/std@0.71.0/http/file_server.ts".to_string(),
      "checksum-1".to_string(),
    );
    assert!(lockfile.resolve_write_bytes().is_some());
  }

  #[test]
  fn returns_the_correct_value_as_json_even_after_writing() {
    let file_path =
      std::env::current_dir().unwrap().join("valid_lockfile.json");
    let lockfile_json = r#"{
  "version": "3",
  "remote": {}
}
"#;
    let mut lockfile =
      Lockfile::with_lockfile_content(file_path, lockfile_json, false).unwrap();

    // Change lockfile
    lockfile.insert_remote(
      "https://deno.land/std@0.71.0/textproto/mod.ts".to_string(),
      "checksum-1".to_string(),
    );
    // Assert it changed
    assert_ne!(lockfile.to_json(), lockfile_json);
    // Assert that to_json returns the changed lockfile even after writing it
    lockfile.resolve_write_bytes();
    assert_ne!(lockfile.to_json(), lockfile_json);
  }

  #[test]
  fn does_always_write_bytes_if_overwrite_is_set() {
    let mut lockfile = setup(true).unwrap();
    assert!(lockfile.resolve_write_bytes().is_some());
  }

  #[test]
  fn does_not_write_bytes_if_overwrite_is_not_set_and_there_are_no_changes() {
    let mut lockfile = setup(false).unwrap();
    assert!(lockfile.resolve_write_bytes().is_none());
  }

  #[test]
  fn does_write_bytes_if_there_are_changes() {
    let mut lockfile = setup(false).unwrap();
    lockfile.insert_remote(
      "https://deno.land/std@0.71.0/http/file_server.ts".to_string(),
      "checksum-1".to_string(),
    );
    assert!(lockfile.resolve_write_bytes().is_some());
  }

  #[test]
  fn does_not_write_bytes_if_all_changes_were_already_written() {
    let mut lockfile = setup(false).unwrap();
    lockfile.insert_remote(
      "https://deno.land/std@0.71.0/http/file_server.ts".to_string(),
      "checksum-1".to_string(),
    );
    assert!(lockfile.resolve_write_bytes().is_some());
    assert!(lockfile.resolve_write_bytes().is_none());
  }

  // // TODO: Currently we always write, when overwrite is set, even if we already wrote the changes before. I think it would be more sane, if we only wrote, when there are unwritten changes. This would probably also mean, that we could just remove the overwrite flag and replace it by setting `has_content_changed` to true, when a lockfile is created with overwrite.
  // #[test]
  // fn does_not_write_bytes_if_overwrite_was_set_but_already_written() {
  //   let mut lockfile = setup(true).unwrap();
  //   assert!(lockfile.resolve_write_bytes().is_some());
  //   assert!(lockfile.resolve_write_bytes().is_none());
  // }

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
  "version": "4",
  "redirects": {
    "https://deno.land/x/std/mod.ts": "https://deno.land/std@0.190.0/mod.ts"
  }
}"#,
      false,
    )
    .unwrap();
    lockfile.insert_redirect(
      "https://deno.land/x/other/mod.ts".to_string(),
      "https://deno.land/x/other@0.1.0/mod.ts".to_string(),
    );
    assert_eq!(
      lockfile.to_json(),
      r#"{
  "version": "4",
  "redirects": {
    "https://deno.land/x/other/mod.ts": "https://deno.land/x/other@0.1.0/mod.ts",
    "https://deno.land/x/std/mod.ts": "https://deno.land/std@0.190.0/mod.ts"
  }
}
"#,
    );
  }

  #[test]
  fn test_version_does_not_change_if_lockfile_did_not_change() {
    let original_content = r#"{
  "version": "3",
  "redirects": {
    "https://deno.land/x/std/mod.ts": "https://deno.land/std@0.190.0/mod.ts"
  },
  "remote": {}
}"#;
    let mut lockfile = Lockfile::with_lockfile_content(
      PathBuf::from("/foo/deno.lock"),
      original_content,
      false,
    )
    .unwrap();
    // Insert already existing redirect
    lockfile.insert_redirect(
      "https://deno.land/x/std/mod.ts".to_string(),
      "https://deno.land/std@0.190.0/mod.ts".to_string(),
    );
    assert!(!lockfile.has_content_changed());
    assert_eq!(lockfile.to_json(), original_content,);
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
      lockfile.to_json(),
      r#"{
  "version": "4",
  "redirects": {
    "https://deno.land/x/std/mod.ts": "https://deno.land/std@0.190.1/mod.ts",
    "https://deno.land/x/std/other.ts": "https://deno.land/std@0.190.1/other.ts"
  }
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
      lockfile.to_json(),
      r#"{
  "version": "4",
  "specifiers": {
    "jsr:@foo/bar@^2": "jsr:@foo/bar@2.1.2",
    "jsr:path": "jsr:@std/path@0.75.1"
  }
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
    assert_eq!(lockfile.content.version, "4");
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
    assert_eq!(lockfile.content.version, "4");
    assert_eq!(lockfile.content.npm.len(), 2);
    assert_eq!(
      lockfile.content.specifiers,
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
    lockfile.insert_jsr_package("dep".to_string(), "integrity".to_string());
    // has changed even though it was empty
    assert!(lockfile.has_content_changed);

    // now try inserting the same package
    lockfile.has_content_changed = false;
    lockfile.insert_jsr_package("dep".to_string(), "integrity".to_string());
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
      "Unable to read lockfile. Lockfile was empty at 'lockfile.json'."
    );
  }

  #[test]
  fn should_maintain_changed_false_flag_when_adding_a_workspace_to_an_empty_lockfile(
  ) {
    // should maintain the has_content_changed flag when lockfile empty
    let mut lockfile = Lockfile::new(PathBuf::from("./deno.lock"), false);

    assert!(!lockfile.has_content_changed());
    lockfile.set_workspace_config(SetWorkspaceConfigOptions {
      no_config: false,
      no_npm: false,
      config: WorkspaceConfig {
        root: WorkspaceMemberConfig {
          dependencies: BTreeSet::from(["jsr:@scope/package".to_string()]),
          package_json_deps: Default::default(),
        },
        members: BTreeMap::new(),
      },
    });
    assert!(!lockfile.has_content_changed()); // should not have changed
  }

  #[test]
  fn should_maintain_changed_true_flag_when_adding_a_workspace_to_an_empty_lockfile(
  ) {
    // should maintain has_content_changed flag when true and lockfile is empty
    let mut lockfile = Lockfile::new(PathBuf::from("./deno.lock"), false);
    lockfile.insert_redirect("a".to_string(), "b".to_string());
    lockfile.remove_redirect("a");

    lockfile.set_workspace_config(SetWorkspaceConfigOptions {
      no_config: false,
      no_npm: false,
      config: WorkspaceConfig {
        root: WorkspaceMemberConfig {
          dependencies: BTreeSet::from(["jsr:@scope/package2".to_string()]),
          package_json_deps: Default::default(),
        },
        members: BTreeMap::new(),
      },
    });
    assert!(lockfile.has_content_changed());
  }

  #[test]
  fn should_be_changed_if_a_workspace_is_added_and_the_lockfile_is_not_emtpy() {
    // should not maintain the has_content_changed flag when lockfile is not empty
    let mut lockfile = Lockfile::new(PathBuf::from("./deno.lock"), true);
    lockfile.insert_redirect("a".to_string(), "b".to_string());
    // Reset has_content_changed flag by writing
    lockfile.resolve_write_bytes();
    assert!(!lockfile.has_content_changed());

    lockfile.set_workspace_config(SetWorkspaceConfigOptions {
      no_config: false,
      no_npm: false,
      config: WorkspaceConfig {
        root: WorkspaceMemberConfig {
          dependencies: BTreeSet::from(["jsr:@scope/package".to_string()]),
          package_json_deps: Default::default(),
        },
        members: BTreeMap::new(),
      },
    });

    assert!(lockfile.has_content_changed()); // should have changed since lockfile was not empty
  }

  #[test]
  fn should_be_changed_if_a_dep_is_removed_from_the_workspace() {
    // Setup
    let mut lockfile = Lockfile::new(PathBuf::from("./deno.lock"), true);
    lockfile.insert_jsr_package("beta".to_string(), "checksum".to_string());
    lockfile.set_workspace_config(SetWorkspaceConfigOptions {
      no_config: false,
      no_npm: false,
      config: WorkspaceConfig {
        root: Default::default(),
        members: BTreeMap::from([(
          "thing".into(),
          WorkspaceMemberConfig {
            dependencies: BTreeSet::from(["beta".into()]),
            package_json_deps: BTreeSet::new(),
          },
        )]),
      },
    });
    lockfile.resolve_write_bytes();
    assert!(!lockfile.has_content_changed());

    lockfile.set_workspace_config(SetWorkspaceConfigOptions {
      no_config: false,
      no_npm: false,
      config: WorkspaceConfig {
        root: Default::default(),
        members: BTreeMap::new(),
      },
    });
    assert!(lockfile.has_content_changed());
  }

  #[test]
  fn should_be_changed_if_a_dep_is_moved_workspace_root_to_a_member_a() {
    // Setup
    let mut lockfile = Lockfile::new(PathBuf::from("./deno.lock"), true);
    lockfile.insert_jsr_package("beta".to_string(), "checksum".to_string());
    lockfile.set_workspace_config(SetWorkspaceConfigOptions {
      no_config: false,
      no_npm: false,
      config: WorkspaceConfig {
        root: WorkspaceMemberConfig {
          dependencies: BTreeSet::from(["beta".into()]),
          package_json_deps: BTreeSet::new(),
        },
        members: BTreeMap::from([("thing".into(), Default::default())]),
      },
    });
    lockfile.resolve_write_bytes();
    assert!(!lockfile.has_content_changed());

    lockfile.set_workspace_config(SetWorkspaceConfigOptions {
      no_config: false,
      no_npm: false,
      config: WorkspaceConfig {
        root: Default::default(),
        members: BTreeMap::from([(
          "thing".into(),
          WorkspaceMemberConfig {
            dependencies: BTreeSet::from(["beta".into()]),
            package_json_deps: BTreeSet::new(),
          },
        )]),
      },
    });
    assert!(lockfile.has_content_changed());
  }

  #[test]
  fn should_be_changed_if_a_dep_is_moved_workspace_root_to_a_member_b() {
    // Setup
    let mut lockfile = Lockfile::new(PathBuf::from("./deno.lock"), true);
    lockfile.insert_jsr_package("beta".to_string(), "checksum".to_string());
    lockfile.set_workspace_config(SetWorkspaceConfigOptions {
      no_config: false,
      no_npm: false,
      config: WorkspaceConfig {
        root: WorkspaceMemberConfig {
          dependencies: BTreeSet::from(["beta".into()]),
          package_json_deps: BTreeSet::new(),
        },
        members: Default::default(),
      },
    });
    lockfile.resolve_write_bytes();
    assert!(!lockfile.has_content_changed());

    lockfile.set_workspace_config(SetWorkspaceConfigOptions {
      no_config: false,
      no_npm: false,
      config: WorkspaceConfig {
        root: Default::default(),
        members: BTreeMap::from([(
          "thing".into(),
          WorkspaceMemberConfig {
            dependencies: BTreeSet::from(["beta".into()]),
            package_json_deps: BTreeSet::new(),
          },
        )]),
      },
    });
    assert!(lockfile.has_content_changed());
  }

  #[test]
  fn should_preserve_workspace_on_no_npm() {
    // Setup
    let mut lockfile = Lockfile::new(PathBuf::from("./deno.lock"), true);
    lockfile.insert_jsr_package("alpha".to_string(), "checksum".to_string());
    lockfile.insert_jsr_package("beta".to_string(), "checksum".to_string());
    lockfile.insert_jsr_package("gamma".to_string(), "checksum".to_string());
    lockfile.set_workspace_config(SetWorkspaceConfigOptions {
      no_config: false,
      no_npm: false,
      config: WorkspaceConfig {
        root: WorkspaceMemberConfig {
          dependencies: BTreeSet::from(["alpha".into()]),
          package_json_deps: BTreeSet::new(),
        },
        members: BTreeMap::from([(
          "thing".into(),
          WorkspaceMemberConfig {
            dependencies: BTreeSet::from(["beta".into()]),
            package_json_deps: BTreeSet::from(["gamma".into()]),
          },
        )]),
      },
    });
    lockfile.resolve_write_bytes();
    assert!(!lockfile.has_content_changed());

    lockfile.set_workspace_config(SetWorkspaceConfigOptions {
      no_config: true,
      no_npm: false,
      config: WorkspaceConfig {
        root: Default::default(),
        members: Default::default(),
      },
    });
    assert!(!lockfile.has_content_changed());
  }
}
