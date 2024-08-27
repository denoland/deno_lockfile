// Copyright 2018-2024 the Deno authors. MIT license.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::HashMap;

use deno_semver::package::PackageReq;
use serde::Serialize;

use crate::JsrPackageInfo;
use crate::LockfileContent;
use crate::NpmPackageInfo;
use crate::WorkspaceConfigContent;

#[derive(Serialize)]
struct SerializedJsrPkg<'a> {
  integrity: &'a str,
  #[serde(skip_serializing_if = "Vec::is_empty")]
  dependencies: Vec<Cow<'a, str>>,
}

#[derive(Serialize)]
struct SerializedNpmPkg<'a> {
  integrity: &'a str,
  #[serde(skip_serializing_if = "Vec::is_empty")]
  dependencies: Vec<Cow<'a, str>>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct SerializedLockfilePackageJsonContent {
  #[serde(default)]
  #[serde(skip_serializing_if = "Vec::is_empty")]
  pub dependencies: Vec<String>,
}

impl SerializedLockfilePackageJsonContent {
  pub fn is_empty(&self) -> bool {
    self.dependencies.is_empty()
  }
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct SerializedWorkspaceMemberConfigContent {
  #[serde(skip_serializing_if = "Vec::is_empty")]
  #[serde(default)]
  pub dependencies: Vec<String>,
  #[serde(skip_serializing_if = "SerializedLockfilePackageJsonContent::is_empty")]
  #[serde(default)]
  pub package_json: SerializedLockfilePackageJsonContent,
}

impl SerializedWorkspaceMemberConfigContent {
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

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct SerializedWorkspaceConfigContent<'a> {
  #[serde(default, flatten)]
  pub root: SerializedWorkspaceMemberConfigContent,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  #[serde(default)]
  pub members: BTreeMap<String, SerializedWorkspaceMemberConfigContent>,
}

impl SerializedWorkspaceConfigContent {
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

#[derive(Serialize)]
struct LockfileV4<'a> {
  // order these based on auditability
  version: &'static str,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  specifiers: BTreeMap<String, &'a str>,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  jsr: BTreeMap<&'a str, SerializedJsrPkg<'a>>,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  npm: BTreeMap<&'a str, SerializedNpmPkg<'a>>,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  redirects: &'a BTreeMap<String, String>,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  remote: &'a BTreeMap<String, String>,
  #[serde(skip_serializing_if = "SerializedWorkspaceConfigContent::is_empty")]
  workspace: &'a SerializedWorkspaceConfigContent,
}

pub fn print_v4_content(content: &LockfileContent) -> String {
  fn handle_jsr<'a>(
    jsr: &'a BTreeMap<String, JsrPackageInfo>,
    jsr_specifiers: &HashMap<PackageReq, String>,
    npm_specifiers: &HashMap<PackageReq, String>,
  ) -> BTreeMap<&'a str, SerializedJsrPkg<'a>> {
    fn create_had_multiple_specifiers_map(
      specifiers: &HashMap<PackageReq, String>,
    ) -> HashMap<&str, bool> {
      let mut had_multiple_specifiers: HashMap<&str, bool> =
        HashMap::with_capacity(specifiers.len());
      for req in specifiers.keys() {
        had_multiple_specifiers
          .entry(&req.name)
          .and_modify(|v| *v = true)
          .or_default();
      }
      had_multiple_specifiers
    }

    let jsr_pkg_had_multiple_specifiers = create_had_multiple_specifiers_map(jsr_specifiers);
    let npm_pkg_had_multiple_specifiers = create_had_multiple_specifiers_map(npm_specifiers);

    jsr
      .iter()
      .map(|(key, value)| {
        (
          key.as_str(),
          SerializedJsrPkg {
            integrity: &value.integrity,
            dependencies: {
              let mut dependencies = value
              .dependencies
              .iter()
              .filter_map(|dep| {
                let had_multiple_specifiers = match dep.kind {
                    deno_semver::package::PackageKind::Jsr => &jsr_pkg_had_multiple_specifiers,
                    deno_semver::package::PackageKind::Npm => &npm_pkg_had_multiple_specifiers,
                };
                let has_single_specifier = had_multiple_specifiers
                  .get(dep.req.name.as_str())
                  .map(|had_multiple| !had_multiple)
                  .unwrap_or(false);
                if has_single_specifier {
                  Some(Cow::Borrowed(dep.req.name.as_str()))
                } else {
                  Some(Cow::Owned(dep.to_string()))
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
    npm: &BTreeMap<String, NpmPackageInfo>,
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

    npm
      .iter()
      .map(|(key, value)| {
        (
          key.as_str(),
          SerializedNpmPkg {
            integrity: &value.integrity,
            dependencies: value
              .dependencies
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
              .collect(),
          },
        )
      })
      .collect()
  }

  fn handle_workspace(content: &WorkspaceConfigContent) -> SerializedWorkspaceConfigContent {
    SerializedWorkspaceConfigContent {
      root: SerializedWorkspaceMemberConfigContent {
        dependencies: todo!(),
        package_json: todo!(),
    },
      members: todo!(),
    }
  }

  let mut specifiers = BTreeMap::new();
  for (key, value) in &content.packages.jsr_specifiers {
    specifiers.insert(key.to_string(), value.as_str());
  }
  for (key, value) in &content.packages.npm_specifiers {
    specifiers.insert(key.to_string(), value.as_str());
  }

  let lockfile = LockfileV4 {
    version: "4",
    specifiers,
    jsr: handle_jsr(&content.packages.jsr, &content.packages.jsr_specifiers, &content.packages.npm_specifiers),
    npm: handle_npm(&content.packages.npm),
    redirects: &content.redirects,
    remote: &content.remote,
    workspace: handle_workspace(&content.workspace),
  };
  serde_json::to_string_pretty(&lockfile).unwrap()
}
