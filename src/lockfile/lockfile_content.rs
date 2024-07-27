// Copyright 2018-2024 the Deno authors. MIT license.

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;

use super::WorkspaceConfigContent;
use crate::printer::print_v4_content;
use crate::DeserializationError;

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
  #[serde(default)]
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

#[derive(Debug, Clone, Serialize, Hash)]
#[serde(rename_all = "camelCase")]
pub struct LockfileContent {
  /// The lockfile version
  pub(crate) version: String,
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
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  #[serde(default)]
  pub redirects: BTreeMap<String, String>,
  // todo(dsherret): in the next lockfile version we should skip
  // serializing this when it's empty
  /// Mapping between URLs and their checksums for "http:" and "https:" deps
  #[serde(default)]
  pub(crate) remote: BTreeMap<String, String>,
  #[serde(skip_serializing_if = "WorkspaceConfigContent::is_empty")]
  #[serde(default)]
  pub(crate) workspace: WorkspaceConfigContent,
}

impl LockfileContent {
  /// Parse the content of a JSON string representing a lockfile in the latest version
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

    #[derive(Debug, Deserialize)]
    struct RawNpmPackageInfo {
      pub integrity: String,
      #[serde(default)]
      pub dependencies: Vec<String>,
    }

    #[derive(Debug, Deserialize)]
    struct RawJsrPackageInfo {
      pub integrity: String,
      #[serde(default)]
      pub dependencies: Vec<String>,
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
      return Ok(Self::empty());
    };

    // TODO: This code is just copied from the previous implementation, that allowed parsing old lockfiles. It can probably be significantly simplified.
    let (jsr, specifiers, npm) = {
      let specifiers: BTreeMap<String, String> =
        deserialize_section(&mut json, "specifiers")?;
      let mut npm: BTreeMap<String, NpmPackageInfo> = Default::default();
      let raw_npm: BTreeMap<String, RawNpmPackageInfo> =
        deserialize_section(&mut json, "npm")?;
      if !raw_npm.is_empty() {
        // collect the versions
        let mut version_by_dep_name: HashMap<String, String> =
          HashMap::with_capacity(raw_npm.len());
        for id in raw_npm.keys() {
          let Some((name, version)) = extract_nv_from_id(id) else {
            return Err(DeserializationError::InvalidNpmPackageId(
              id.to_string(),
            ));
          };
          version_by_dep_name.insert(name.to_string(), version.to_string());
        }

        // now go through and create the resolved npm package information
        for (key, value) in raw_npm {
          let mut dependencies = BTreeMap::new();
          for dep in value.dependencies {
            let (left, right) = match extract_nv_from_id(&dep) {
              Some((name, version)) => (name, version),
              None => match version_by_dep_name.get(&dep) {
                Some(version) => (dep.as_str(), version.as_str()),
                None => return Err(DeserializationError::MissingPackage(dep)),
              },
            };
            let (key, package_name, version) = match right.strip_prefix("npm:")
            {
              Some(right) => {
                // ex. key@npm:package-a@version
                match extract_nv_from_id(right) {
                  Some((package_name, version)) => {
                    (left, package_name, version)
                  }
                  None => {
                    return Err(
                      DeserializationError::InvalidNpmPackageDependency(
                        dep.to_string(),
                      ),
                    );
                  }
                }
              }
              None => (left, left, right),
            };
            dependencies
              .insert(key.to_string(), format!("{}@{}", package_name, version));
          }
          npm.insert(
            key,
            NpmPackageInfo {
              integrity: value.integrity,
              dependencies,
            },
          );
        }
      }
      let mut jsr: BTreeMap<String, JsrPackageInfo> = Default::default();
      {
        let raw_jsr: BTreeMap<String, RawJsrPackageInfo> =
          deserialize_section(&mut json, "jsr")?;
        if !raw_jsr.is_empty() {
          // collect the specifier information
          let mut to_resolved_specifiers: HashMap<&str, Option<&str>> =
            HashMap::with_capacity(specifiers.len() * 2);
          // first insert the specifiers that should be left alone
          for specifier in specifiers.keys() {
            to_resolved_specifiers.insert(specifier, None);
          }
          // then insert the mapping specifiers
          for specifier in specifiers.keys() {
            let Some((name, req)) = split_pkg_req(specifier) else {
              return Err(DeserializationError::InvalidPackageSpecifier(
                specifier.to_string(),
              ));
            };
            if req.is_some() {
              let entry = to_resolved_specifiers.entry(name);
              // if an entry is occupied that means there's multiple specifiers
              // for the same name, such as one without a req, so ignore inserting
              // here
              if let std::collections::hash_map::Entry::Vacant(entry) = entry {
                entry.insert(Some(specifier));
              }
            }
          }

          // now go through the dependencies mapping to the new ones
          for (key, value) in raw_jsr {
            let mut dependencies = BTreeSet::new();
            for dep in value.dependencies {
              let Some(maybe_specifier) =
                to_resolved_specifiers.get(dep.as_str())
              else {
                todo!();
              };
              dependencies
                .insert(maybe_specifier.map(|s| s.to_string()).unwrap_or(dep));
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

      (jsr, specifiers, npm)
    };

    Ok(LockfileContent {
      version: json
        .remove("version")
        .and_then(|v| match v {
          Value::String(v) => Some(v),
          _ => None,
        })
        .unwrap_or_else(|| "3".to_string()),
      jsr,
      specifiers,
      npm,
      redirects: deserialize_section(&mut json, "redirects")?,
      remote: deserialize_section(&mut json, "remote")?,
      workspace: deserialize_section(&mut json, "workspace")?,
    })
  }

  /// Convert the lockfile content to a v4 lockfile
  ///
  /// You should probably use [Lockfile::]
  pub fn to_json(&self) -> String {
    // TODO: Think about adding back support for older lockfile versions
    let mut text = String::new();
    print_v4_content(self, &mut text);
    text
  }

  pub fn empty() -> Self {
    Self {
      version: "4".to_string(),
      redirects: Default::default(),
      remote: BTreeMap::new(),
      workspace: Default::default(),
      jsr: Default::default(),
      specifiers: Default::default(),
      npm: Default::default(),
    }
  }

  pub fn is_empty(&self) -> bool {
    self.jsr.is_empty()
      && self.npm.is_empty()
      && self.specifiers.is_empty()
      && self.redirects.is_empty()
      && self.remote.is_empty()
      && self.workspace.is_empty()
  }
}
