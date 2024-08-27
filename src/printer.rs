// Copyright 2018-2024 the Deno authors. MIT license.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::HashMap;

use deno_semver::jsr::JsrDepPackageReq;
use deno_semver::package::PackageReq;
use serde::Serialize;

use crate::JsrPackageInfo;
use crate::LockfileContent;
use crate::LockfilePackageJsonContent;
use crate::NpmPackageInfo;
use crate::WorkspaceConfigContent;
use crate::WorkspaceMemberConfigContent;

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
struct SerializedLockfilePackageJsonContent<'a> {
  #[serde(default)]
  #[serde(skip_serializing_if = "Vec::is_empty")]
  pub dependencies: Vec<&'a PackageReq>,
}

impl<'a> SerializedLockfilePackageJsonContent<'a> {
  pub fn is_empty(&self) -> bool {
    self.dependencies.is_empty()
  }
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct SerializedWorkspaceMemberConfigContent<'a> {
  #[serde(skip_serializing_if = "Vec::is_empty")]
  #[serde(default)]
  pub dependencies: Vec<&'a JsrDepPackageReq>,
  #[serde(
    skip_serializing_if = "SerializedLockfilePackageJsonContent::is_empty"
  )]
  #[serde(default)]
  pub package_json: SerializedLockfilePackageJsonContent<'a>,
}

impl<'a> SerializedWorkspaceMemberConfigContent<'a> {
  pub fn is_empty(&self) -> bool {
    self.dependencies.is_empty() && self.package_json.is_empty()
  }
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct SerializedWorkspaceConfigContent<'a> {
  #[serde(default, flatten)]
  pub root: SerializedWorkspaceMemberConfigContent<'a>,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  #[serde(default)]
  pub members: BTreeMap<&'a str, SerializedWorkspaceMemberConfigContent<'a>>,
}

impl<'a> SerializedWorkspaceConfigContent<'a> {
  pub fn is_empty(&self) -> bool {
    self.root.is_empty() && self.members.is_empty()
  }
}

#[derive(Serialize)]
struct LockfileV4<'a> {
  // order these based on auditability
  version: &'static str,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  specifiers: BTreeMap<&'a JsrDepPackageReq, &'a String>,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  jsr: BTreeMap<&'a str, SerializedJsrPkg<'a>>,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  npm: BTreeMap<&'a str, SerializedNpmPkg<'a>>,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  redirects: &'a BTreeMap<String, String>,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  remote: &'a BTreeMap<String, String>,
  #[serde(skip_serializing_if = "SerializedWorkspaceConfigContent::is_empty")]
  workspace: SerializedWorkspaceConfigContent<'a>,
}

pub fn print_v4_content(content: &LockfileContent) -> String {
  fn handle_jsr<'a>(
    jsr: &'a BTreeMap<String, JsrPackageInfo>,
    specifiers: &HashMap<JsrDepPackageReq, String>,
  ) -> BTreeMap<&'a str, SerializedJsrPkg<'a>> {
    fn create_had_multiple_specifiers_map(
      specifiers: &HashMap<JsrDepPackageReq, String>,
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
          key.as_str(),
          SerializedJsrPkg {
            integrity: &value.integrity,
            dependencies: {
              let mut dependencies = value
                .dependencies
                .iter()
                .filter_map(|dep| {
                  let has_single_specifier = pkg_had_multiple_specifiers
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

  fn handle_pkg_json_content(
    content: &LockfilePackageJsonContent,
  ) -> SerializedLockfilePackageJsonContent {
    let mut dependencies = content.dependencies.iter().collect::<Vec<_>>();
    dependencies.sort();
    SerializedLockfilePackageJsonContent { dependencies }
  }

  fn handle_workspace_member(
    member: &WorkspaceMemberConfigContent,
  ) -> SerializedWorkspaceMemberConfigContent {
    SerializedWorkspaceMemberConfigContent {
      dependencies: {
        let mut member = member.dependencies.iter().collect::<Vec<_>>();
        member.sort();
        member
      },
      package_json: handle_pkg_json_content(&member.package_json),
    }
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
    }
  }

  // insert sorted
  let mut specifiers = BTreeMap::new();
  for (key, value) in &content.packages.specifiers {
    specifiers.insert(key, value);
  }

  let lockfile = LockfileV4 {
    version: "4",
    specifiers,
    jsr: handle_jsr(&content.packages.jsr, &content.packages.specifiers),
    npm: handle_npm(&content.packages.npm),
    redirects: &content.redirects,
    remote: &content.remote,
    workspace: handle_workspace(&content.workspace),
  };
  serde_json::to_string_pretty(&lockfile).unwrap()
}
