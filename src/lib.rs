// Copyright 2018-2024 the Deno authors. All rights reserved. MIT license.

mod error;

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;

use indexmap::IndexMap;
use ring::digest;
use serde::Deserialize;
use serde::Serialize;

mod transforms;

pub use error::LockfileError as Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct LockfileNpmPackageId(String);

struct LockfileNpmGraphPackage {
  reference_count: usize,
  integrity: String,
  dependencies: BTreeMap<String, LockfileNpmPackageId>,
}

struct LockfileNpmGraph {
  pub root_packages: HashMap<String, LockfileNpmPackageId>,
  pub packages: HashMap<LockfileNpmPackageId, LockfileNpmGraphPackage>,
}

impl LockfileNpmGraph {
  pub fn from_lockfile(content: &PackagesContent) -> Self {
    let mut root_packages =
      HashMap::<String, LockfileNpmPackageId>::with_capacity(
        content.specifiers.len(),
      );
    // collect the specifiers to version mappings
    for (key, value) in &content.specifiers {
      if let Some(key) = key.strip_prefix("npm:") {
        if let Some(value) = value.strip_prefix("npm:") {
          root_packages
            .insert(key.to_string(), LockfileNpmPackageId(value.to_string()));
        }
      }
    }

    let mut packages = HashMap::new();
    for (id, package) in &content.npm {
      packages.insert(
        LockfileNpmPackageId(id.clone()),
        LockfileNpmGraphPackage {
          reference_count: 0,
          integrity: package.integrity.clone(),
          dependencies: package
            .dependencies
            .iter()
            .map(|(key, dep_id)| {
              (key.clone(), LockfileNpmPackageId(dep_id.clone()))
            })
            .collect(),
        },
      );
    }

    let mut visited = HashSet::new();
    let mut pending = root_packages.values().cloned().collect::<VecDeque<_>>();
    while let Some(id) = pending.pop_back() {
      if let Some(package) = packages.get_mut(&id) {
        package.reference_count += 1;
        if visited.insert(id) {
          for dep_id in package.dependencies.values() {
            pending.push_back(dep_id.clone());
          }
        }
      }
    }

    Self {
      root_packages,
      packages,
    }
  }

  pub fn remove_root_package(&mut self, package_req: &String) {
    let mut pending = VecDeque::new();
    if let Some(package_id) = self.root_packages.remove(package_req) {
      pending.push_back(package_id);
    }

    while let Some(id) = pending.pop_back() {
      eprintln!("HANDLING: {}", id.0);
      if let Some(package) = self.packages.get_mut(&id) {
        package.reference_count -= 1;
        if package.reference_count == 0 {
          for dep_id in package.dependencies.values() {
            pending.push_back(dep_id.clone());
          }
          self.packages.remove(&id);
        }
      }
    }
  }

  fn populate_packages(self, packages: &mut PackagesContent) {
    for (req, id) in self.root_packages {
      eprintln!("ADDING: {:#?}", req);
      packages
        .specifiers
        .insert(format!("npm:{}", req), format!("npm:{}", id.0));
    }
    for (id, package) in self.packages {
      eprintln!("ADDING: {}", id.0);
      packages.npm.insert(
        id.0,
        NpmPackageInfo {
          integrity: package.integrity.clone(),
          dependencies: package
            .dependencies
            .into_iter()
            .map(|(name, id)| (name, id.0))
            .collect(),
        },
      );
    }
  }
}

pub struct NpmPackageLockfileInfo {
  pub display_id: String,
  pub serialized_id: String,
  pub integrity: String,
  pub dependencies: Vec<NpmPackageDependencyLockfileInfo>,
}

pub struct NpmPackageDependencyLockfileInfo {
  pub name: String,
  pub id: String,
}

fn gen_checksum(v: &[impl AsRef<[u8]>]) -> String {
  let mut ctx = digest::Context::new(&digest::SHA256);
  for src in v {
    ctx.update(src.as_ref());
  }
  let digest = ctx.finish();
  let out: Vec<String> = digest
    .as_ref()
    .iter()
    .map(|byte| format!("{byte:02x}"))
    .collect();
  out.join("")
}

#[derive(Debug)]
pub struct LockfileError(String);

impl std::fmt::Display for LockfileError {
  fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    f.write_str(&self.0)
  }
}

impl std::error::Error for LockfileError {}

#[derive(Debug, Clone, Serialize, Deserialize, Hash)]
pub struct NpmPackageInfo {
  pub integrity: String,
  pub dependencies: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, Hash)]
pub struct PackagesContent {
  /// Mapping between requests for deno specifiers and resolved packages, eg.
  /// {
  ///   "deno:path": "deno:@std/path@1.0.0",
  ///   "deno:ts-morph@11": "npm:ts-morph@11.0.0",
  ///   "deno:@foo/bar@^2.1": "deno:@foo/bar@2.1.3",
  ///   "npm:@ts-morph/common@^11": "npm:@ts-morph/common@11.0.0",
  /// }
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  #[serde(default)]
  pub specifiers: BTreeMap<String, String>,

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
    self.specifiers.is_empty() && self.npm.is_empty()
  }

  pub fn clear_npm(&mut self) {
    self.specifiers.retain(|k, v| {
      let has_npm = k.starts_with("npm:") || v.starts_with("npm:");
      !has_npm
    });
    self.npm.clear();
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockfileImportMap {
  #[serde(skip_serializing_if = "IndexMap::is_empty")]
  #[serde(default)]
  pub imports: IndexMap<String, String>,
  #[serde(skip_serializing_if = "IndexMap::is_empty")]
  #[serde(default)]
  pub scopes: IndexMap<String, IndexMap<String, String>>,
}

// IndexMap doesn't implement Hash, so we need to do this ourselves.
// Hashing is used in the LSP to tell when the lockfile has changed.
impl std::hash::Hash for LockfileImportMap {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    for (key, value) in &self.imports {
      key.hash(state);
      value.hash(state);
    }
    for (key, scope_map) in &self.scopes {
      key.hash(state);
      for (scope_key, scope_value) in scope_map {
        scope_key.hash(state);
        scope_value.hash(state);
      }
    }
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
  /// Mapping between URLs and their checksums for "http:" and "https:" deps
  remote: BTreeMap<String, String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  #[serde(default)]
  pub import_map: Option<LockfileImportMap>,
  #[serde(skip_serializing_if = "Option::is_none")]
  #[serde(default)]
  pub package_json: Option<Vec<String>>,
}

impl LockfileContent {
  fn empty() -> Self {
    Self {
      version: "3".to_string(),
      packages: Default::default(),
      redirects: Default::default(),
      remote: BTreeMap::new(),
      import_map: None,
      package_json: None,
    }
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
  /// Create a new [`Lockfile`] instance from a given filename. The content of
  /// the file is read from the filesystem using [`std::fs::read_to_string`].
  pub fn new(filename: PathBuf, overwrite: bool) -> Result<Lockfile, Error> {
    // Writing a lock file always uses the new format.
    if overwrite {
      return Ok(Lockfile {
        overwrite,
        has_content_changed: false,
        content: LockfileContent::empty(),
        filename,
      });
    }

    let result = match std::fs::read_to_string(&filename) {
      Ok(content) => Ok(content),
      Err(e) => {
        if e.kind() == std::io::ErrorKind::NotFound {
          return Ok(Lockfile {
            overwrite,
            has_content_changed: false,
            content: LockfileContent::empty(),
            filename,
          });
        } else {
          Err(e)
        }
      }
    };

    let s =
      result.map_err(|_| Error::ReadError(filename.display().to_string()))?;

    Self::with_lockfile_content(filename, &s, overwrite)
  }

  /// Create a new [`Lockfile`] instance from given filename and its content.
  pub fn with_lockfile_content(
    filename: PathBuf,
    content: &str,
    overwrite: bool,
  ) -> Result<Lockfile, Error> {
    eprintln!("NEW LOCKFILE: {} {}", filename.display(), overwrite);
    // Writing a lock file always uses the new format.
    if overwrite {
      return Ok(Lockfile {
        overwrite,
        has_content_changed: false,
        content: LockfileContent::empty(),
        filename,
      });
    }

    let value: serde_json::Map<String, serde_json::Value> =
      serde_json::from_str(content)
        .map_err(|_| Error::ParseError(filename.display().to_string()))?;
    let version = value.get("version").and_then(|v| v.as_str());
    let value = match version {
      Some("3") => value,
      Some("2") => transforms::transform2_to_3(value),
      None => transforms::transform2_to_3(transforms::transform1_to_2(value)),
      Some(version) => {
        return Err(Error::ParseError(format!(
          "Unsupported lockfile version '{}'. Try upgrading Deno or recreating the lockfile.",
          version
        )))
      }
    };
    let content = serde_json::from_value::<LockfileContent>(value.into())
      .map_err(|err| {
        eprintln!("ERROR: {:#}", err);
        Error::ParseError(filename.display().to_string())
      })?;

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

  pub fn set_package_json_deps(
    &mut self,
    package_json_deps: Option<Vec<String>>,
  ) {
    if let Some(new_package_json_deps) = package_json_deps {
      match &mut self.content.package_json {
        Some(current_package_json) => {
          if new_package_json_deps != *current_package_json {
            let current_package_json =
              std::mem::replace(current_package_json, new_package_json_deps);
            let mut graph =
              LockfileNpmGraph::from_lockfile(&self.content.packages);
            for package_req in &current_package_json {
              eprintln!("LOOKING AT: {}", package_req);
              graph.remove_root_package(&package_req);
            }

            // clear out the npm packages
            eprintln!("CLEARING OUT");
            self.content.packages.clear_npm();
            // now populate the graph back into the packages
            eprintln!("POPULATING");
            graph.populate_packages(&mut self.content.packages);

            self.has_content_changed = true;
            eprintln!("RESULT: {}", self.as_json_string());
          }
        }
        None => {
          self.content.package_json = Some(new_package_json_deps);
          // clear out all npm related specifiers when adding a package.json
          self.content.packages.clear_npm();
          self.has_content_changed = true;
        }
      }
    } else {
      // don't clear the package.json field because someone might
      // be running a one-off script without a package.json
    }
  }

  // Synchronize lock file to disk - noop if --lock-write file is not specified.
  pub fn write(&self) -> Result<(), Error> {
    if !self.has_content_changed && !self.overwrite {
      return Ok(());
    }
    eprintln!("WRITING");
    eprintln!("RESULT: {}", self.as_json_string());

    let mut f = std::fs::OpenOptions::new()
      .write(true)
      .create(true)
      .truncate(true)
      .open(&self.filename)?;
    f.write_all(self.as_json_string().as_bytes())?;
    Ok(())
  }

  // TODO(bartlomieju): this function should return an error instead of a bool,
  // but it requires changes to `deno_graph`'s `Locker`.
  pub fn check_or_insert_remote(
    &mut self,
    specifier: &str,
    code: &str,
  ) -> bool {
    if !(specifier.starts_with("http:") || specifier.starts_with("https:")) {
      return true;
    }
    if self.overwrite {
      // In case --lock-write is specified check always passes
      self.insert(specifier, code);
      true
    } else {
      self.check_or_insert(specifier, code)
    }
  }

  pub fn check_or_insert_npm_package(
    &mut self,
    package_info: NpmPackageLockfileInfo,
  ) -> Result<(), LockfileError> {
    if self.overwrite {
      // In case --lock-write is specified check always passes
      self.insert_npm_package(package_info);
      Ok(())
    } else {
      self.check_or_insert_npm(package_info)
    }
  }

  /// Checks the given module is included, if so verify the checksum. If module
  /// is not included, insert it.
  fn check_or_insert(&mut self, specifier: &str, code: &str) -> bool {
    if let Some(lockfile_checksum) = self.content.remote.get(specifier) {
      let compiled_checksum = gen_checksum(&[code.as_bytes()]);
      lockfile_checksum == &compiled_checksum
    } else {
      self.insert(specifier, code);
      true
    }
  }

  fn insert(&mut self, specifier: &str, code: &str) {
    let checksum = gen_checksum(&[code.as_bytes()]);
    self.content.remote.insert(specifier.to_string(), checksum);
    self.has_content_changed = true;
  }

  fn check_or_insert_npm(
    &mut self,
    package: NpmPackageLockfileInfo,
  ) -> Result<(), LockfileError> {
    if let Some(package_info) =
      self.content.packages.npm.get(&package.serialized_id)
    {
      if package_info.integrity.as_str() != package.integrity {
        return Err(LockfileError(format!(
            "Integrity check failed for npm package: \"{}\". Unable to verify that the package
is the same as when the lockfile was generated.

This could be caused by:
  * the lock file may be corrupt
  * the source itself may be corrupt

Use \"--lock-write\" flag to regenerate the lockfile at \"{}\".",
            package.display_id, self.filename.display()
          )));
      }
    } else {
      self.insert_npm_package(package);
    }

    Ok(())
  }

  fn insert_npm_package(&mut self, package_info: NpmPackageLockfileInfo) {
    let dependencies = package_info
      .dependencies
      .iter()
      .map(|dep| (dep.name.to_string(), dep.id.to_string()))
      .collect::<BTreeMap<String, String>>();

    self.content.packages.npm.insert(
      package_info.serialized_id.to_string(),
      NpmPackageInfo {
        integrity: package_info.integrity,
        dependencies,
      },
    );
    self.has_content_changed = true;
  }

  pub fn insert_package_specifier(
    &mut self,
    serialized_package_req: String,
    serialized_package_id: String,
  ) {
    let maybe_prev = self
      .content
      .packages
      .specifiers
      .get(&serialized_package_req);

    if maybe_prev.is_none() || maybe_prev != Some(&serialized_package_id) {
      self.has_content_changed = true;
      self
        .content
        .packages
        .specifiers
        .insert(serialized_package_req, serialized_package_id);
    }
  }

  pub fn insert_redirect(&mut self, from: String, to: String) {
    let maybe_prev = self.content.redirects.get(&from);

    if maybe_prev.is_none() || maybe_prev != Some(&to) {
      self.has_content_changed = true;
      self.content.redirects.insert(from, to);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use pretty_assertions::assert_eq;
  use std::fs::File;
  use std::io::prelude::*;
  use std::io::Write;
  use temp_dir::TempDir;

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

  fn setup(temp_dir: &TempDir) -> PathBuf {
    let file_path = temp_dir.path().join("valid_lockfile.json");
    let mut file = File::create(file_path).expect("write file fail");

    file.write_all(LOCKFILE_JSON.as_bytes()).unwrap();

    temp_dir.path().join("valid_lockfile.json")
  }

  #[test]
  fn create_lockfile_for_nonexistent_path() {
    let file_path = PathBuf::from("nonexistent_lock_file.json");
    assert!(Lockfile::new(file_path, false).is_ok());
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
      "Unable to parse contents of lockfile. Unsupported lockfile version '2000'. Try upgrading Deno or recreating the lockfile.".to_string()
    );
  }

  #[test]
  fn new_valid_lockfile() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = setup(&temp_dir);

    let result = Lockfile::new(file_path, false).unwrap();

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
    let temp_dir = TempDir::new().unwrap();
    let file_path = setup(&temp_dir);

    let mut lockfile = Lockfile::new(file_path, false).unwrap();

    lockfile.insert(
      "https://deno.land/std@0.71.0/io/util.ts",
      "Here is some source code",
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
    let temp_dir = TempDir::new().unwrap();
    let file_path = setup(&temp_dir);

    let mut lockfile = Lockfile::new(file_path, true).unwrap();

    lockfile.insert(
      "https://deno.land/std@0.71.0/textproto/mod.ts",
      "Here is some source code",
    );
    lockfile.insert(
      "https://deno.land/std@0.71.0/io/util.ts",
      "more source code here",
    );
    lockfile.insert(
      "https://deno.land/std@0.71.0/async/delay.ts",
      "this source is really exciting",
    );

    lockfile.write().expect("unable to write");

    let file_path_buf = temp_dir.path().join("valid_lockfile.json");
    let file_path = file_path_buf.to_str().expect("file path fail").to_string();

    // read the file contents back into a string and check
    let mut checkfile = File::open(file_path).expect("Unable to open the file");
    let mut contents = String::new();
    checkfile
      .read_to_string(&mut contents)
      .expect("Unable to read the file");

    let contents_json =
      serde_json::from_str::<serde_json::Value>(&contents).unwrap();
    let object = contents_json["remote"].as_object().unwrap();

    assert_eq!(
      object
        .get("https://deno.land/std@0.71.0/textproto/mod.ts")
        .and_then(|v| v.as_str()),
      // sha-256 hash of the source 'Here is some source code'
      Some("fedebba9bb82cce293196f54b21875b649e457f0eaf55556f1e318204947a28f")
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
    let temp_dir = TempDir::new().unwrap();
    let file_path = setup(&temp_dir);

    let mut lockfile = Lockfile::new(file_path, false).unwrap();

    lockfile.insert(
      "https://deno.land/std@0.71.0/textproto/mod.ts",
      "Here is some source code",
    );

    let check_true = lockfile.check_or_insert_remote(
      "https://deno.land/std@0.71.0/textproto/mod.ts",
      "Here is some source code",
    );
    assert!(check_true);

    let check_false = lockfile.check_or_insert_remote(
      "https://deno.land/std@0.71.0/textproto/mod.ts",
      "Here is some NEW source code",
    );
    assert!(!check_false);

    // Not present in lockfile yet, should be inserted and check passed.
    let check_true = lockfile.check_or_insert_remote(
      "https://deno.land/std@0.71.0/http/file_server.ts",
      "This is new Source code",
    );
    assert!(check_true);
  }

  #[test]
  fn check_or_insert_lockfile_npm() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = setup(&temp_dir);

    let mut lockfile = Lockfile::new(file_path, false).unwrap();

    let npm_package = NpmPackageLockfileInfo {
      display_id: "nanoid@3.3.4".to_string(),
      serialized_id: "nanoid@3.3.4".to_string(),
      integrity: "sha512-MqBkQh/OHTS2egovRtLk45wEyNXwF+cokD+1YPf9u5VfJiRdAiRwB2froX5Co9Rh20xs4siNPm8naNotSD6RBw==".to_string(),
      dependencies: vec![],
    };
    let check_ok = lockfile.check_or_insert_npm_package(npm_package);
    assert!(check_ok.is_ok());

    let npm_package = NpmPackageLockfileInfo {
      display_id: "picocolors@1.0.0".to_string(),
      serialized_id: "picocolors@1.0.0".to_string(),
      integrity: "sha512-1fygroTLlHu66zi26VoTDv8yRgm0Fccecssto+MhsZ0D/DGW2sm8E8AjW7NU5VVTRt5GxbeZ5qBuJr+HyLYkjQ==".to_string(),
      dependencies: vec![],
    };
    // Integrity is borked in the loaded lockfile
    let check_err = lockfile.check_or_insert_npm_package(npm_package);
    assert!(check_err.is_err());

    let npm_package = NpmPackageLockfileInfo {
      display_id: "source-map-js@1.0.2".to_string(),
      serialized_id: "source-map-js@1.0.2".to_string(),
      integrity: "sha512-R0XvVJ9WusLiqTCEiGCmICCMplcCkIwwR11mOSD9CR5u+IXYdiseeEuXCVAjS54zqwkLcPNnmU4OeJ6tUrWhDw==".to_string(),
      dependencies: vec![],
    };
    // Not present in lockfile yet, should be inserted and check passed.
    let check_ok = lockfile.check_or_insert_npm_package(npm_package);
    assert!(check_ok.is_ok());

    let npm_package = NpmPackageLockfileInfo {
      display_id: "source-map-js@1.0.2".to_string(),
      serialized_id: "source-map-js@1.0.2".to_string(),
      integrity: "sha512-foobar".to_string(),
      dependencies: vec![],
    };
    // Now present in lockfile, should file due to borked integrity
    let check_err = lockfile.check_or_insert_npm_package(npm_package);
    assert!(check_err.is_err());
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
  fn test_insert_deno() {
    let mut lockfile = Lockfile::with_lockfile_content(
      PathBuf::from("/foo/deno.lock"),
      r#"{
  "version": "3",
  "packages": {
    "specifiers": {
      "deno:path": "deno:@std/path@0.75.0"
    }
  },
  "remote": {}
}"#,
      false,
    )
    .unwrap();
    lockfile.insert_package_specifier(
      "deno:path".to_string(),
      "deno:@std/path@0.75.0".to_string(),
    );
    assert!(!lockfile.has_content_changed);
    lockfile.insert_package_specifier(
      "deno:path".to_string(),
      "deno:@std/path@0.75.1".to_string(),
    );
    assert!(lockfile.has_content_changed);
    lockfile.insert_package_specifier(
      "deno:@foo/bar@^2".to_string(),
      "deno:@foo/bar@2.1.2".to_string(),
    );
    assert_eq!(
      lockfile.as_json_string(),
      r#"{
  "version": "3",
  "packages": {
    "specifiers": {
      "deno:@foo/bar@^2": "deno:@foo/bar@2.1.2",
      "deno:path": "deno:@std/path@0.75.1"
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
}
