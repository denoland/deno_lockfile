// Copyright 2018-2024 the Deno authors. MIT license.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::HashMap;

use serde::Serialize;

use crate::JsrPackageInfo;
use crate::LockfileContent;
use crate::NpmPackageInfo;
use crate::WorkspaceConfigContent;

#[derive(Serialize)]
struct SerializedJsrPkg<'a> {
  integrity: &'a str,
  #[serde(skip_serializing_if = "Vec::is_empty")]
  dependencies: Vec<&'a str>,
}

#[derive(Serialize)]
struct SerializedNpmPkg<'a> {
  integrity: &'a str,
  #[serde(skip_serializing_if = "Vec::is_empty")]
  dependencies: Vec<Cow<'a, str>>,
}

#[derive(Serialize)]
struct LockfileV4<'a> {
  // order these based on auditability
  version: &'static str,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  specifiers: &'a BTreeMap<String, String>,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  jsr: BTreeMap<&'a str, SerializedJsrPkg<'a>>,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  npm: BTreeMap<&'a str, SerializedNpmPkg<'a>>,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  redirects: &'a BTreeMap<String, String>,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  remote: &'a BTreeMap<String, String>,
  #[serde(skip_serializing_if = "WorkspaceConfigContent::is_empty")]
  workspace: &'a WorkspaceConfigContent,
}

pub fn print_v4_content(content: &LockfileContent) -> String {
  fn handle_jsr<'a>(
    jsr: &'a BTreeMap<String, JsrPackageInfo>,
    specifiers: &BTreeMap<String, String>,
  ) -> BTreeMap<&'a str, SerializedJsrPkg<'a>> {
    fn split_pkg_req(value: &str) -> Option<(&str, Option<&str>)> {
      if value.len() < 5 {
        return None;
      }
      // 5 is length of `jsr:@`/`npm:@`
      let Some(at_index) = value[5..].find('@').map(|i| i + 5) else {
        // no version requirement
        // ex. `npm:jsonc-parser` or `jsr:@pkg/scope`
        return Some((value, None));
      };
      let name = &value[..at_index];
      let version = &value[at_index + 1..];
      Some((name, Some(version)))
    }

    let mut pkg_had_multiple_specifiers: HashMap<&str, bool> =
      HashMap::with_capacity(specifiers.len());
    for req in specifiers.keys() {
      let Some((name, _)) = split_pkg_req(req) else {
        continue; // corrupt
      };
      pkg_had_multiple_specifiers
        .entry(name)
        .and_modify(|v| *v = true)
        .or_default();
    }

    jsr
      .iter()
      .map(|(key, value)| {
        (
          key.as_str(),
          SerializedJsrPkg {
            integrity: &value.integrity,
            dependencies: value
              .dependencies
              .iter()
              .filter_map(|dep| {
                let (name, _req) = split_pkg_req(dep)?;
                let has_single_specifier = pkg_had_multiple_specifiers
                  .get(name)
                  .map(|had_multiple| !had_multiple)
                  .unwrap_or(false);
                if has_single_specifier {
                  Some(name)
                } else {
                  Some(dep)
                }
              })
              .collect(),
          },
        )
      })
      .collect()
  }

  fn handle_npm<'a>(
    npm: &'a BTreeMap<String, NpmPackageInfo>,
  ) -> BTreeMap<&'a str, SerializedNpmPkg> {
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

  let lockfile = LockfileV4 {
    version: "4",
    specifiers: &content.packages.specifiers,
    jsr: handle_jsr(&content.packages.jsr, &content.packages.specifiers),
    npm: handle_npm(&content.packages.npm),
    redirects: &content.redirects,
    remote: &content.remote,
    workspace: &content.workspace,
  };
  serde_json::to_string_pretty(&lockfile).unwrap()
}
