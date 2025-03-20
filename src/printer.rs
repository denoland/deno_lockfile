// Copyright 2018-2024 the Deno authors. MIT license.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;

use deno_semver::jsr::JsrDepPackageReq;
use deno_semver::package::PackageNv;
use deno_semver::SmallStackString;
use deno_semver::StackString;
use serde::Serialize;

use crate::JsrPackageInfo;
use crate::LockfileContent;
use crate::LockfilePackageJsonContent;
use crate::LockfilePatchContent;
use crate::NpmPackageInfo;
use crate::WorkspaceConfigContent;
use crate::WorkspaceMemberConfigContent;

#[derive(Serialize)]
struct SerializedJsrPkg<'a> {
  integrity: &'a str,
  #[serde(skip_serializing_if = "Vec::is_empty")]
  dependencies: Vec<StackString>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SerializedNpmPkg<'a> {
  /// Will be `None` for patch packages.
  #[serde(skip_serializing_if = "Option::is_none")]
  integrity: Option<&'a str>,
  #[serde(skip_serializing_if = "Vec::is_empty")]
  dependencies: Vec<Cow<'a, str>>,
  #[serde(skip_serializing_if = "Vec::is_empty")]
  optional_dependencies: Vec<Cow<'a, str>>,
  #[serde(skip_serializing_if = "Vec::is_empty")]
  os: Vec<SmallStackString>,
  #[serde(skip_serializing_if = "Vec::is_empty")]
  cpu: Vec<SmallStackString>,

  #[serde(skip_serializing_if = "Option::is_none")]
  tarball: Option<&'a str>,
}

// WARNING: It's important to implement Ord/PartialOrd on the final
// normalized string so that sorting works according to the final
// output and so that's why this is used rather than JsrDepPackageReq
// directly.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct SerializedJsrDepPackageReq(StackString);

impl SerializedJsrDepPackageReq {
  pub fn new(dep_req: &JsrDepPackageReq) -> Self {
    Self(dep_req.to_string_normalized())
  }
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct SerializedLockfilePackageJsonContent {
  #[serde(default)]
  #[serde(skip_serializing_if = "Vec::is_empty")]
  pub dependencies: Vec<SerializedJsrDepPackageReq>,
}

impl SerializedLockfilePackageJsonContent {
  pub fn is_empty(&self) -> bool {
    self.dependencies.is_empty()
  }
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct SerializedLockfilePatchContent {
  #[serde(default)]
  #[serde(skip_serializing_if = "Vec::is_empty")]
  pub dependencies: Vec<SerializedJsrDepPackageReq>,
  #[serde(default)]
  #[serde(skip_serializing_if = "Vec::is_empty")]
  pub peer_dependencies: Vec<SerializedJsrDepPackageReq>,
  #[serde(default)]
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  pub peer_dependencies_meta: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct SerializedWorkspaceMemberConfigContent {
  #[serde(skip_serializing_if = "Vec::is_empty")]
  #[serde(default)]
  pub dependencies: Vec<SerializedJsrDepPackageReq>,
  #[serde(
    skip_serializing_if = "SerializedLockfilePackageJsonContent::is_empty"
  )]
  #[serde(default)]
  pub package_json: SerializedLockfilePackageJsonContent,
}

impl SerializedWorkspaceMemberConfigContent {
  pub fn is_empty(&self) -> bool {
    self.dependencies.is_empty() && self.package_json.is_empty()
  }
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct SerializedWorkspaceConfigContent<'a> {
  #[serde(default, flatten)]
  pub root: SerializedWorkspaceMemberConfigContent,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  #[serde(default)]
  pub members: BTreeMap<&'a str, SerializedWorkspaceMemberConfigContent>,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  #[serde(default)]
  pub patches: BTreeMap<&'a str, SerializedLockfilePatchContent>,
}

impl SerializedWorkspaceConfigContent<'_> {
  pub fn is_empty(&self) -> bool {
    self.root.is_empty() && self.members.is_empty() && self.patches.is_empty()
  }
}

#[derive(Serialize)]
struct LockfileV5<'a> {
  // order these based on auditability
  version: &'static str,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  specifiers: BTreeMap<SerializedJsrDepPackageReq, &'a str>,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  jsr: BTreeMap<&'a PackageNv, SerializedJsrPkg<'a>>,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  npm: BTreeMap<&'a str, SerializedNpmPkg<'a>>,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  redirects: &'a BTreeMap<String, String>,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  remote: &'a BTreeMap<String, String>,
  #[serde(skip_serializing_if = "SerializedWorkspaceConfigContent::is_empty")]
  workspace: SerializedWorkspaceConfigContent<'a>,
}

pub fn print_v5_content(content: &LockfileContent) -> String {
  fn handle_jsr<'a>(
    jsr: &'a BTreeMap<PackageNv, JsrPackageInfo>,
    specifiers: &HashMap<JsrDepPackageReq, SmallStackString>,
  ) -> BTreeMap<&'a PackageNv, SerializedJsrPkg<'a>> {
    fn create_had_multiple_specifiers_map(
      specifiers: &HashMap<JsrDepPackageReq, SmallStackString>,
    ) -> HashMap<&str, bool> {
      let mut had_multiple_specifiers: HashMap<&str, bool> =
        HashMap::with_capacity(specifiers.len());
      for dep in specifiers.keys() {
        had_multiple_specifiers
          .entry(&dep.req.name)
          .and_modify(|v| *v = true)
          .or_default();
      }
      had_multiple_specifiers
    }

    let pkg_had_multiple_specifiers =
      create_had_multiple_specifiers_map(specifiers);

    jsr
      .iter()
      .map(|(key, value)| {
        (
          key,
          SerializedJsrPkg {
            integrity: &value.integrity,
            dependencies: {
              let mut dependencies = value
                .dependencies
                .iter()
                .map(|dep| {
                  let has_single_specifier = pkg_had_multiple_specifiers
                    .get(dep.req.name.as_str())
                    .map(|had_multiple| !had_multiple)
                    .unwrap_or(false);
                  if has_single_specifier {
                    let mut stack_string = StackString::with_capacity(
                      dep.kind.scheme_with_colon().len() + dep.req.name.len(),
                    );
                    stack_string.push_str(dep.kind.scheme_with_colon());
                    stack_string.push_str(dep.req.name.as_str());
                    stack_string
                  } else {
                    dep.to_string_normalized()
                  }
                })
                .collect::<Vec<_>>();
              dependencies.sort();
              dependencies
            },
          },
        )
      })
      .collect()
  }

  fn handle_npm(
    npm: &BTreeMap<StackString, NpmPackageInfo>,
  ) -> BTreeMap<&str, SerializedNpmPkg> {
    fn extract_nv_from_id(value: &str) -> Option<(&str, &str)> {
      if value.is_empty() {
        return None;
      }
      let at_index = value[1..].find('@').map(|i| i + 1)?;
      let name = &value[..at_index];
      let version = &value[at_index + 1..];
      Some((name, version))
    }

    let mut pkg_had_multiple_versions: HashMap<&str, bool> =
      HashMap::with_capacity(npm.len());
    for id in npm.keys() {
      let Some((name, _)) = extract_nv_from_id(id) else {
        continue; // corrupt
      };
      pkg_had_multiple_versions
        .entry(name)
        .and_modify(|v| *v = true)
        .or_default();
    }

    fn handle_deps<'a>(
      deps: &'a BTreeMap<StackString, StackString>,
      pkg_had_multiple_versions: &HashMap<&str, bool>,
    ) -> Vec<Cow<'a, str>> {
      deps
        .iter()
        .filter_map(|(key, id)| {
          let (name, version) = extract_nv_from_id(id)?;
          if name == key {
            let has_single_version = pkg_had_multiple_versions
              .get(name)
              .map(|had_multiple| !had_multiple)
              .unwrap_or(false);
            if has_single_version {
              Some(Cow::Borrowed(name))
            } else {
              Some(Cow::Borrowed(id))
            }
          } else {
            Some(Cow::Owned(format!("{}@npm:{}@{}", key, name, version)))
          }
        })
        .collect::<Vec<_>>()
    }

    npm
      .iter()
      .map(|(key, value)| {
        let dependencies =
          handle_deps(&value.dependencies, &pkg_had_multiple_versions);
        let optional_dependencies =
          handle_deps(&value.optional_dependencies, &pkg_had_multiple_versions);
        (
          key.as_str(),
          SerializedNpmPkg {
            integrity: value.integrity.as_deref(),
            dependencies,
            optional_dependencies,
            os: value.os.clone(),
            cpu: value.cpu.clone(),
            tarball: value.tarball.as_deref(),
          },
        )
      })
      .collect()
  }

  fn handle_pkg_json_content(
    content: &LockfilePackageJsonContent,
  ) -> SerializedLockfilePackageJsonContent {
    SerializedLockfilePackageJsonContent {
      dependencies: sort_deps(&content.dependencies),
    }
  }

  fn handle_workspace_member(
    member: &WorkspaceMemberConfigContent,
  ) -> SerializedWorkspaceMemberConfigContent {
    SerializedWorkspaceMemberConfigContent {
      dependencies: sort_deps(&member.dependencies),
      package_json: handle_pkg_json_content(&member.package_json),
    }
  }

  fn handle_patch_content(
    content: &LockfilePatchContent,
  ) -> SerializedLockfilePatchContent {
    SerializedLockfilePatchContent {
      dependencies: sort_deps(&content.dependencies),
      peer_dependencies: sort_deps(&content.peer_dependencies),
      peer_dependencies_meta: content
        .peer_dependencies_meta
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect(),
    }
  }

  fn sort_deps(
    deps: &HashSet<JsrDepPackageReq>,
  ) -> Vec<SerializedJsrDepPackageReq> {
    let mut dependencies = deps
      .iter()
      .map(SerializedJsrDepPackageReq::new)
      .collect::<Vec<_>>();
    dependencies.sort();
    dependencies
  }

  fn handle_workspace(
    content: &WorkspaceConfigContent,
  ) -> SerializedWorkspaceConfigContent {
    SerializedWorkspaceConfigContent {
      root: handle_workspace_member(&content.root),
      members: content
        .members
        .iter()
        .map(|(key, value)| (key.as_str(), handle_workspace_member(value)))
        .collect(),
      patches: content
        .patches
        .iter()
        .map(|(key, value)| (key.as_str(), handle_patch_content(value)))
        .collect(),
    }
  }

  // insert sorted
  let mut specifiers = BTreeMap::new();
  for (key, value) in &content.packages.specifiers {
    // insert a string to ensure proper sorting
    specifiers.insert(SerializedJsrDepPackageReq::new(key), value.as_str());
  }

  let lockfile = LockfileV5 {
    version: "5",
    specifiers,
    jsr: handle_jsr(&content.packages.jsr, &content.packages.specifiers),
    npm: handle_npm(&content.packages.npm),
    redirects: &content.redirects,
    remote: &content.remote,
    workspace: handle_workspace(&content.workspace),
  };
  serde_json::to_string_pretty(&lockfile).unwrap()
}
