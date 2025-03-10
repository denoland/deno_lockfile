// Copyright 2018-2024 the Deno authors. MIT license.

#![deny(clippy::print_stderr)]
#![deny(clippy::print_stdout)]

mod error;
mod graphs;

use std::borrow::Cow;
use std::collections::btree_map::Entry as BTreeMapEntry;
use std::collections::hash_map::Entry as HashMapEntry;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;

use deno_semver::jsr::JsrDepPackageReq;
use deno_semver::package::PackageNv;
use deno_semver::SmallStackString;
use deno_semver::StackString;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde::Serialize;

mod printer;
mod transforms;

pub use error::DeserializationError;
pub use error::LockfileError;
pub use error::LockfileErrorReason;

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

#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceConfig {
  pub root: WorkspaceMemberConfig,
  pub members: HashMap<String, WorkspaceMemberConfig>,
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMemberConfig {
  pub dependencies: HashSet<JsrDepPackageReq>,
  pub package_json_deps: HashSet<JsrDepPackageReq>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpmPackageLockfileInfo {
  pub serialized_id: StackString,
  pub integrity: String,
  pub dependencies: Vec<NpmPackageDependencyLockfileInfo>,
  pub optional_dependencies: Vec<usize>,
  pub scripts: bool,
  pub deprecated: bool,
  pub bin: bool,
  pub os: Vec<SmallStackString>,
  pub cpu: Vec<SmallStackString>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpmPackageDependencyLockfileInfo {
  pub name: StackString,
  pub id: StackString,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct NpmPackageInfo {
  pub integrity: String,
  #[serde(default)]
  pub dependencies: BTreeMap<StackString, StackString>,
  #[serde(default)]
  pub optional_dependencies: BTreeSet<StackString>,
  pub scripts: bool,
  pub bin: bool,
  pub deprecated: bool,
  pub os: Vec<SmallStackString>,
  pub cpu: Vec<SmallStackString>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct NpmPackageDist {
  pub shasum: String,
  pub integrity: Option<String>,
}

#[derive(Debug, Clone)]
pub struct JsrPackageInfo {
  pub integrity: String,
  /// List of package requirements found in the dependency.
  ///
  /// This is used to tell when a package can be removed from the lockfile.
  pub dependencies: HashSet<JsrDepPackageReq>,
}

#[derive(Clone, Debug, Default)]
pub struct PackagesContent {
  /// Mapping between requests for jsr specifiers and resolved packages, eg.
  /// {
  ///   "jsr:@foo/bar@^2.1": "2.1.3",
  ///   "npm:@ts-morph/common@^11": "11.0.0",
  ///   "npm:@ts-morph/common@^12": "12.0.0__some-peer-dep@1.0.0",
  /// }
  pub specifiers: HashMap<JsrDepPackageReq, SmallStackString>,

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
  pub jsr: BTreeMap<PackageNv, JsrPackageInfo>,

  /// Mapping between resolved npm specifiers and their associated info, eg.
  /// {
  ///   "chalk@5.0.0": {
  ///     "integrity": "sha512-...",
  ///     "dependencies": {
  ///       "ansi-styles": "ansi-styles@4.1.0",
  ///     }
  ///   }
  /// }
  pub npm: BTreeMap<StackString, NpmPackageInfo>,
}

impl PackagesContent {
  fn is_empty(&self) -> bool {
    self.specifiers.is_empty() && self.npm.is_empty() && self.jsr.is_empty()
  }
}

#[derive(Debug, Default, Clone, Deserialize)]
pub(crate) struct LockfilePackageJsonContent {
  pub dependencies: HashSet<JsrDepPackageReq>,
}

impl LockfilePackageJsonContent {
  pub fn is_empty(&self) -> bool {
    self.dependencies.is_empty()
  }
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceMemberConfigContent {
  #[serde(default)]
  pub dependencies: HashSet<JsrDepPackageReq>,
  #[serde(default)]
  pub package_json: LockfilePackageJsonContent,
}

impl WorkspaceMemberConfigContent {
  pub fn is_empty(&self) -> bool {
    self.dependencies.is_empty() && self.package_json.is_empty()
  }

  pub fn dep_reqs(&self) -> impl Iterator<Item = &JsrDepPackageReq> {
    self
      .package_json
      .dependencies
      .iter()
      .chain(self.dependencies.iter())
  }
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceConfigContent {
  #[serde(default, flatten)]
  pub root: WorkspaceMemberConfigContent,
  #[serde(default)]
  pub members: HashMap<String, WorkspaceMemberConfigContent>,
}

impl WorkspaceConfigContent {
  pub fn is_empty(&self) -> bool {
    self.root.is_empty() && self.members.is_empty()
  }

  fn get_all_dep_reqs(&self) -> impl Iterator<Item = &JsrDepPackageReq> {
    self
      .root
      .dep_reqs()
      .chain(self.members.values().flat_map(|m| m.dep_reqs()))
  }
}

#[derive(Debug, Default, Clone)]
pub struct LockfileContent {
  pub packages: PackagesContent,
  pub redirects: BTreeMap<String, String>,
  /// Mapping between URLs and their checksums for "http:" and "https:" deps
  pub(crate) remote: BTreeMap<String, String>,
  pub(crate) workspace: WorkspaceConfigContent,
}

impl LockfileContent {
  pub fn from_json(
    json: serde_json::Value,
  ) -> Result<Self, DeserializationError> {
    fn extract_nv_from_id(value: &str) -> Option<(&str, &str)> {
      if value.is_empty() {
        return None;
      }
      let at_index = value[1..].find('@').map(|i| i + 1)?;
      let name = &value[..at_index];
      let version = &value[at_index + 1..];
      Some((name, version))
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RawNpmPackageInfo {
      pub integrity: String,
      #[serde(default)]
      pub dependencies: Vec<StackString>,
      #[serde(default)]
      pub optional_dependencies: Vec<usize>,
      #[serde(default)]
      pub os: Vec<SmallStackString>,
      #[serde(default)]
      pub cpu: Vec<SmallStackString>,
      #[serde(default)]
      pub deprecated: bool,
      #[serde(default)]
      pub scripts: bool,
      #[serde(default)]
      pub bin: bool,
    }

    #[derive(Debug, Deserialize)]
    struct RawJsrPackageInfo {
      pub integrity: String,
      #[serde(default)]
      pub dependencies: Vec<StackString>,
    }

    fn deserialize_section<T: DeserializeOwned + Default>(
      json: &mut serde_json::Map<String, serde_json::Value>,
      key: &'static str,
    ) -> Result<T, DeserializationError> {
      match json.remove(key) {
        Some(value) => serde_json::from_value(value)
          .map_err(|err| DeserializationError::FailedDeserializing(key, err)),
        None => Ok(Default::default()),
      }
    }

    use serde_json::Value;

    let Value::Object(mut json) = json else {
      return Ok(Self::default());
    };

    Ok(LockfileContent {
      packages: {
        let deserialized_specifiers: BTreeMap<StackString, SmallStackString> =
          deserialize_section(&mut json, "specifiers")?;
        let mut specifiers =
          HashMap::with_capacity(deserialized_specifiers.len());
        for (key, value) in deserialized_specifiers {
          let dep = JsrDepPackageReq::from_str_loose(&key)?;
          specifiers.insert(dep, value);
        }

        let mut npm: BTreeMap<StackString, NpmPackageInfo> = Default::default();
        let raw_npm: BTreeMap<StackString, RawNpmPackageInfo> =
          deserialize_section(&mut json, "npm")?;
        if !raw_npm.is_empty() {
          // collect the versions
          let mut version_by_dep_name: HashMap<StackString, StackString> =
            HashMap::with_capacity(raw_npm.len());
          for id in raw_npm.keys() {
            let Some((name, version)) = extract_nv_from_id(id) else {
              return Err(DeserializationError::InvalidNpmPackageId(
                id.clone(),
              ));
            };
            version_by_dep_name.insert(name.into(), version.into());
          }

          // now go through and create the resolved npm package information
          for (key, value) in raw_npm {
            let mut dependencies: BTreeMap<StackString, StackString> =
              BTreeMap::new();
            let mut optional_dependencies = BTreeSet::new();
            let optional_set = value
              .optional_dependencies
              .into_iter()
              .collect::<BTreeSet<_>>();
            for (idx, dep) in value.dependencies.into_iter().enumerate() {
              let (left, right) = match extract_nv_from_id(&dep) {
                Some((name, version)) => (name, version),
                None => match version_by_dep_name.get(&dep) {
                  Some(version) => (dep.as_str(), version.as_str()),
                  None => {
                    return Err(DeserializationError::MissingPackage(dep))
                  }
                },
              };
              let (key, package_name, version) = match right
                .strip_prefix("npm:")
              {
                Some(right) => {
                  // ex. key@npm:package-a@version
                  match extract_nv_from_id(right) {
                    Some((package_name, version)) => {
                      (left, package_name, version)
                    }
                    None => {
                      return Err(
                        DeserializationError::InvalidNpmPackageDependency(dep),
                      );
                    }
                  }
                }
                None => (left, left, right),
              };
              dependencies.insert(key.into(), {
                let mut text = StackString::with_capacity(
                  package_name.len() + 1 + version.len(),
                );
                text.push_str(package_name);
                text.push('@');
                text.push_str(version);
                text
              });
              if optional_set.contains(&idx) {
                optional_dependencies.insert(StackString::from_string(
                  format!("{}@{}", package_name, version),
                ));
              }
            }
            npm.insert(
              key,
              NpmPackageInfo {
                integrity: value.integrity,
                dependencies,
                cpu: value.cpu,
                os: value.os,
                deprecated: value.deprecated,
                scripts: value.scripts,
                optional_dependencies,
                bin: value.bin,
              },
            );
          }
        }
        let mut jsr: BTreeMap<PackageNv, JsrPackageInfo> = Default::default();
        {
          let raw_jsr: BTreeMap<PackageNv, RawJsrPackageInfo> =
            deserialize_section(&mut json, "jsr")?;
          if !raw_jsr.is_empty() {
            // collect the specifier information
            let mut to_resolved_specifiers: HashMap<
              Cow<JsrDepPackageReq>,
              &JsrDepPackageReq,
            > = HashMap::with_capacity(specifiers.len() * 2);
            // first insert the specifiers with the version reqs
            for dep in specifiers.keys() {
              to_resolved_specifiers.insert(Cow::Borrowed(dep), dep);
            }
            // then insert the specifiers without version reqs
            for dep in specifiers.keys() {
              let Ok(dep_no_version_req) = JsrDepPackageReq::from_str(
                &format!("{}{}", dep.kind.scheme_with_colon(), dep.req.name),
              ) else {
                continue; // should never happen
              };
              let entry =
                to_resolved_specifiers.entry(Cow::Owned(dep_no_version_req));
              // if an entry is occupied that means there's multiple specifiers
              // for the same name, such as one without a req, so ignore inserting
              // here
              if let HashMapEntry::Vacant(entry) = entry {
                entry.insert(dep);
              }
            }

            // now go through the dependencies mapping to the new ones
            for (key, value) in raw_jsr {
              let mut dependencies =
                HashSet::with_capacity(value.dependencies.len());
              for dep in value.dependencies {
                let raw_dep = dep;
                let Ok(dep) = JsrDepPackageReq::from_str(&raw_dep) else {
                  continue; // should never happen
                };
                let Some(resolved_dep) = to_resolved_specifiers.get(&dep)
                else {
                  return Err(DeserializationError::InvalidJsrDependency {
                    dependency: raw_dep,
                    package: key,
                  });
                };
                dependencies.insert((*resolved_dep).clone());
              }
              jsr.insert(
                key,
                JsrPackageInfo {
                  integrity: value.integrity,
                  dependencies,
                },
              );
            }
          }
        }

        PackagesContent {
          specifiers,
          jsr,
          npm,
        }
      },
      redirects: deserialize_section(&mut json, "redirects")?,
      remote: deserialize_section(&mut json, "remote")?,
      workspace: deserialize_section(&mut json, "workspace")?,
    })
  }

  pub fn is_empty(&self) -> bool {
    self.packages.is_empty()
      && self.redirects.is_empty()
      && self.remote.is_empty()
      && self.workspace.is_empty()
  }
}

pub struct NewLockfileOptions<'a> {
  pub file_path: PathBuf,
  pub content: &'a str,
  pub overwrite: bool,
}

#[derive(Debug, Clone)]
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
      content: LockfileContent::default(),
      filename,
    }
  }

  pub fn new(opts: NewLockfileOptions) -> Result<Lockfile, Box<LockfileError>> {
    fn load_content(
      content: &str,
    ) -> Result<LockfileContent, LockfileErrorReason> {
      let value: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(content)
          .map_err(LockfileErrorReason::ParseError)?;
      let version = value.get("version").and_then(|v| v.as_str());
      let value = match version {
        Some("4") => value,
        Some("3") => transforms::transform3_to_4(value)?,
        Some("2") => {
          transforms::transform3_to_4(transforms::transform2_to_3(value))?
        }
        None => transforms::transform3_to_4(transforms::transform2_to_3(
          transforms::transform1_to_2(value),
        ))?,
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
    if opts.overwrite {
      return Ok(Lockfile {
        overwrite: opts.overwrite,
        filename: opts.file_path,
        has_content_changed: false,
        content: LockfileContent::default(),
      });
    }

    if opts.content.trim().is_empty() {
      return Err(Box::new(LockfileError {
        file_path: opts.file_path.display().to_string(),
        source: LockfileErrorReason::Empty,
      }));
    }

    let content =
      load_content(opts.content).map_err(|reason| LockfileError {
        file_path: opts.file_path.display().to_string(),
        source: reason,
      })?;
    Ok(Lockfile {
      overwrite: opts.overwrite,
      has_content_changed: false,
      content,
      filename: opts.file_path,
    })
  }

  pub fn as_json_string(&self) -> String {
    let mut text = printer::print_v4_content(&self.content);
    text.reserve(1);
    text.push('\n');
    text
  }

  pub fn set_workspace_config(
    &mut self,
    mut options: SetWorkspaceConfigOptions,
  ) {
    fn update_workspace_member(
      has_content_changed: &mut bool,
      removed_deps: &mut HashSet<JsrDepPackageReq>,
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
        options
          .config
          .root
          .package_json_deps
          .clone_from(&self.content.workspace.root.package_json.dependencies);
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
        options
          .config
          .root
          .dependencies
          .clone_from(&self.content.workspace.root.dependencies);
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
        if !options.config.members.contains_key(key) {
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
      .cloned()
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
        old_deps.into_iter(),
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
  pub fn resolve_write_bytes(&mut self) -> Option<Vec<u8>> {
    if !self.has_content_changed && !self.overwrite {
      return None;
    }

    self.has_content_changed = false;
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
      BTreeMapEntry::Vacant(entry) => {
        entry.insert(hash);
        self.has_content_changed = true;
      }
      BTreeMapEntry::Occupied(mut entry) => {
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
    let optional_dependencies = package_info
      .optional_dependencies
      .into_iter()
      .map(|idx| package_info.dependencies[idx].id.clone())
      .collect();
    let dependencies = package_info
      .dependencies
      .into_iter()
      .map(|dep| (dep.name, dep.id))
      .collect::<BTreeMap<StackString, StackString>>();

    let entry = self.content.packages.npm.entry(package_info.serialized_id);
    let package_info = NpmPackageInfo {
      integrity: package_info.integrity,
      dependencies,
      optional_dependencies,
      scripts: package_info.scripts,
      deprecated: package_info.deprecated,
      os: package_info.os,
      cpu: package_info.cpu,
      bin: package_info.bin,
    };
    match entry {
      BTreeMapEntry::Vacant(entry) => {
        entry.insert(package_info);
        self.has_content_changed = true;
      }
      BTreeMapEntry::Occupied(mut entry) => {
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
    package_req: JsrDepPackageReq,
    serialized_package_id: SmallStackString,
  ) {
    let entry = self.content.packages.specifiers.entry(package_req);
    match entry {
      HashMapEntry::Vacant(entry) => {
        entry.insert(serialized_package_id);
        self.has_content_changed = true;
      }
      HashMapEntry::Occupied(mut entry) => {
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
  pub fn insert_package(&mut self, name: PackageNv, integrity: String) {
    let entry = self.content.packages.jsr.entry(name);
    match entry {
      BTreeMapEntry::Vacant(entry) => {
        entry.insert(JsrPackageInfo {
          integrity,
          dependencies: Default::default(),
        });
        self.has_content_changed = true;
      }
      BTreeMapEntry::Occupied(mut entry) => {
        if *entry.get().integrity != integrity {
          entry.get_mut().integrity = integrity;
          self.has_content_changed = true;
        }
      }
    }
  }

  /// Adds package dependencies of a JSR package. This is only used to track
  /// when packages can be removed from the lockfile.
  ///
  /// Note: You MUST insert the package specifiers for any dependencies before
  /// adding them here as unresolved dependencies will be ignored.
  pub fn add_package_deps(
    &mut self,
    nv: &PackageNv,
    deps: impl Iterator<Item = JsrDepPackageReq>,
  ) {
    if let Some(pkg) = self.content.packages.jsr.get_mut(nv) {
      let start_count = pkg.dependencies.len();
      // don't include unresolved dependendencies
      let resolved_deps =
        deps.filter(|dep| self.content.packages.specifiers.contains_key(dep));
      pkg.dependencies.extend(resolved_deps);
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
      BTreeMapEntry::Vacant(entry) => {
        entry.insert(to);
        self.has_content_changed = true;
      }
      BTreeMapEntry::Occupied(mut entry) => {
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
  use deno_semver::package::PackageReq;
  use pretty_assertions::assert_eq;

  const LOCKFILE_JSON: &str = r#"
{
  "version": "4",
  "npm": {
    "nanoid@3.3.4": {
      "integrity": "sha512-MqBkQh/OHTS2egovRtLk45wEyNXwF+cokD+1YPf9u5VfJiRdAiRwB2froX5Co9Rh20xs4siNPm8naNotSD6RBw==",
      "dependencies": []
      "optional_dependencies": [],
      "os": [],
      "cpu": [],
      "scripts": false,
      "deprecated": false,
    },
    "picocolors@1.0.0": {
      "integrity": "sha512-foobar",
      "dependencies": []
    }
  },
  "remote": {
    "https://deno.land/std@0.71.0/textproto/mod.ts": "3118d7a42c03c242c5a49c2ad91c8396110e14acca1324e7aaefd31a999b71a4",
    "https://deno.land/std@0.71.0/async/delay.ts": "35957d585a6e3dd87706858fb1d6b551cb278271b03f52c5a2cb70e65e00c26a"
  }
}"#;

  fn setup(overwrite: bool) -> Result<Lockfile, Box<LockfileError>> {
    let file_path =
      std::env::current_dir().unwrap().join("valid_lockfile.json");
    Lockfile::new(NewLockfileOptions {
      file_path,
      content: LOCKFILE_JSON,
      overwrite,
    })
  }

  #[test]
  fn future_version_unsupported() {
    let file_path = PathBuf::from("lockfile.json");
    let err = Lockfile::new(NewLockfileOptions {
      file_path,
      content: "{ \"version\": \"2000\" }",
      overwrite: false,
    })
    .unwrap_err();
    match err.source {
      LockfileErrorReason::UnsupportedVersion { version } => {
        assert_eq!(version, "2000")
      }
      _ => unreachable!(),
    }
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
    let result = Lockfile::new(NewLockfileOptions {
      file_path,
      content: LOCKFILE_JSON,
      overwrite: false,
    })
    .unwrap();

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
      serialized_id: "nanoid@3.3.4".into(),
      integrity: "sha512-MqBkQh/OHTS2egovRtLk45wEyNXwF+cokD+1YPf9u5VfJiRdAiRwB2froX5Co9Rh20xs4siNPm8naNotSD6RBw==".to_string(),
      dependencies: vec![],
      optional_dependencies: vec![],
      os: vec![],
      cpu: vec![],
      scripts: false,
      deprecated: false,
      bin: false,
    };
    lockfile.insert_npm_package(npm_package);
    assert!(!lockfile.has_content_changed);

    // insert package that exists already, but has slightly different properties
    let npm_package = NpmPackageLockfileInfo {
      serialized_id: "picocolors@1.0.0".into(),
      integrity: "sha512-1fygroTLlHu66zi26VoTDv8yRgm0Fccecssto+MhsZ0D/DGW2sm8E8AjW7NU5VVTRt5GxbeZ5qBuJr+HyLYkjQ==".to_string(),
      dependencies: vec![],
      optional_dependencies: vec![],
      os: vec![],
      cpu: vec![],
      scripts: false,
      deprecated: false,
      bin: false,
    };
    lockfile.insert_npm_package(npm_package);
    assert!(lockfile.has_content_changed);

    lockfile.has_content_changed = false;
    let npm_package = NpmPackageLockfileInfo {
      serialized_id: "source-map-js@1.0.2".into(),
      integrity: "sha512-R0XvVJ9WusLiqTCEiGCmICCMplcCkIwwR11mOSD9CR5u+IXYdiseeEuXCVAjS54zqwkLcPNnmU4OeJ6tUrWhDw==".to_string(),
      dependencies: vec![],
      optional_dependencies: vec![],
      os: vec![],
      cpu: vec![],
      scripts: false,
      deprecated: false,
      bin: false,
    };
    // Not present in lockfile yet, should be inserted
    lockfile.insert_npm_package(npm_package.clone());
    assert!(lockfile.has_content_changed);
    lockfile.has_content_changed = false;

    // this one should not say the lockfile has changed because it's the same
    lockfile.insert_npm_package(npm_package);
    assert!(!lockfile.has_content_changed);

    let npm_package = NpmPackageLockfileInfo {
      serialized_id: "source-map-js@1.0.2".into(),
      integrity: "sha512-foobar".to_string(),
      dependencies: vec![],
      optional_dependencies: vec![],
      os: vec![],
      cpu: vec![],
      scripts: false,
      deprecated: false,
      bin: false,
    };
    // Now present in lockfile, should be changed due to different integrity
    lockfile.insert_npm_package(npm_package);
    assert!(lockfile.has_content_changed);
  }

  #[test]
  fn lockfile_with_redirects() {
    let mut lockfile = Lockfile::new(NewLockfileOptions {
      file_path: PathBuf::from("/foo/deno.lock"),
      content: r#"{
  "version": "4",
  "redirects": {
    "https://deno.land/x/std/mod.ts": "https://deno.land/std@0.190.0/mod.ts"
  }
}"#,

      overwrite: false,
    })
    .unwrap();
    lockfile.content.redirects.insert(
      "https://deno.land/x/other/mod.ts".to_string(),
      "https://deno.land/x/other@0.1.0/mod.ts".to_string(),
    );
    assert_eq!(
      lockfile.as_json_string(),
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
  fn test_insert_redirect() {
    let mut lockfile = Lockfile::new(NewLockfileOptions {
      file_path: PathBuf::from("/foo/deno.lock"),
      content: r#"{
  "version": "4",
  "redirects": {
    "https://deno.land/x/std/mod.ts": "https://deno.land/std@0.190.0/mod.ts"
  }
}"#,
      overwrite: false,
    })
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
    let mut lockfile = Lockfile::new(NewLockfileOptions {
      file_path: PathBuf::from("/foo/deno.lock"),
      content: r#"{
  "version": "4",
  "specifiers": {
    "jsr:path": "jsr:@std/path@0.75.0"
  }
}"#,
      overwrite: false,
    })
    .unwrap();
    lockfile.insert_package_specifier(
      JsrDepPackageReq::jsr(PackageReq::from_str("path").unwrap()),
      "jsr:@std/path@0.75.0".into(),
    );
    assert!(!lockfile.has_content_changed);
    lockfile.insert_package_specifier(
      JsrDepPackageReq::jsr(PackageReq::from_str("path").unwrap()),
      "jsr:@std/path@0.75.1".into(),
    );
    assert!(lockfile.has_content_changed);
    lockfile.insert_package_specifier(
      JsrDepPackageReq::jsr(PackageReq::from_str("@foo/bar@^2").unwrap()),
      "jsr:@foo/bar@2.1.2".into(),
    );
    assert_eq!(
      lockfile.as_json_string(),
      r#"{
  "version": "4",
  "specifiers": {
    "jsr:@foo/bar@2": "jsr:@foo/bar@2.1.2",
    "jsr:path@*": "jsr:@std/path@0.75.1"
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
    let lockfile = Lockfile::new(NewLockfileOptions {
      file_path,
      content,
      overwrite: false,
    })
    .unwrap();
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
    let lockfile = Lockfile::new(NewLockfileOptions {
      file_path,
      content,
      overwrite: false,
    })
    .unwrap();
    assert_eq!(lockfile.content.packages.npm.len(), 2);
    assert_eq!(
      lockfile.content.packages.specifiers,
      HashMap::from([(
        JsrDepPackageReq::npm(PackageReq::from_str("nanoid").unwrap()),
        "3.3.4".into()
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
    let mut lockfile = Lockfile::new(NewLockfileOptions {
      file_path,
      content,
      overwrite: false,
    })
    .unwrap();

    lockfile.insert_package_specifier(
      JsrDepPackageReq::jsr(PackageReq::from_str("dep2").unwrap()),
      "dep2@1.0.0".into(),
    );
    assert!(lockfile.has_content_changed);
    lockfile.has_content_changed = false;

    assert!(!lockfile.has_content_changed);
    let dep_nv = PackageNv::from_str("dep@1.0.0").unwrap();
    lockfile.insert_package(dep_nv.clone(), "integrity".to_string());
    // has changed even though it was empty
    assert!(lockfile.has_content_changed);

    // now try inserting the same package
    lockfile.has_content_changed = false;
    lockfile.insert_package(dep_nv.clone(), "integrity".to_string());
    assert!(!lockfile.has_content_changed);

    // now with new deps
    lockfile.add_package_deps(
      &dep_nv,
      vec![JsrDepPackageReq::jsr(PackageReq::from_str("dep2").unwrap())]
        .into_iter(),
    );
    assert!(lockfile.has_content_changed);
    lockfile.has_content_changed = false;

    // now insert a dep that doesn't have a package specifier
    lockfile.add_package_deps(
      &dep_nv,
      vec![JsrDepPackageReq::jsr(
        PackageReq::from_str("dep-non-resolved").unwrap(),
      )]
      .into_iter(),
    );
    assert!(!lockfile.has_content_changed);
  }

  #[test]
  fn empty_lockfile_nicer_error() {
    let content: &str = r#"  "#;
    let file_path = PathBuf::from("lockfile.json");
    let err = Lockfile::new(NewLockfileOptions {
      file_path,
      content,
      overwrite: false,
    })
    .err()
    .unwrap();
    assert!(matches!(err.source, LockfileErrorReason::Empty));
  }
}
