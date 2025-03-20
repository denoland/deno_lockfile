use std::{collections::HashMap, future::Future};

use deno_semver::{package::PackageNv, Version};
use serde_json::Value;
use thiserror::Error;

pub type JsonMap = serde_json::Map<String, Value>;

pub fn transform1_to_2(json: JsonMap) -> JsonMap {
  let mut new_map = JsonMap::new();
  new_map.insert("version".to_string(), "2".into());
  new_map.insert("remote".to_string(), json.into());
  new_map
}

pub fn transform2_to_3(mut json: JsonMap) -> JsonMap {
  json.insert("version".into(), "3".into());
  if let Some(Value::Object(mut npm_obj)) = json.remove("npm") {
    let mut new_obj = JsonMap::new();
    if let Some(packages) = npm_obj.remove("packages") {
      new_obj.insert("npm".into(), packages);
    }
    if let Some(Value::Object(specifiers)) = npm_obj.remove("specifiers") {
      let mut new_specifiers = JsonMap::new();
      for (key, value) in specifiers {
        if let Value::String(value) = value {
          new_specifiers
            .insert(format!("npm:{}", key), format!("npm:{}", value).into());
        }
      }
      if !new_specifiers.is_empty() {
        new_obj.insert("specifiers".into(), new_specifiers.into());
      }
    }
    json.insert("packages".into(), new_obj.into());
  }

  json
}

#[derive(Debug, Error)]
pub enum TransformError {
  #[error("Failed extracting npm name and version from dep '{id}'.")]
  FailedExtractingV3NpmDepNv { id: String },
  #[error("Failed getting npm package info: {source}")]
  FailedGettingNpmPackageInfo {
    #[source]
    source: Box<dyn std::error::Error>,
  },
}

// note: although these functions are found elsewhere in this repo,
// it is purposefully duplicated here to ensure it never changes
// for these transforms
fn extract_nv_from_id(value: &str) -> Option<(&str, &str)> {
  if value.is_empty() {
    return None;
  }
  let at_index = value[1..].find('@')? + 1;
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
pub fn transform3_to_4(mut json: JsonMap) -> Result<JsonMap, TransformError> {
  json.insert("version".into(), "4".into());
  if let Some(Value::Object(mut packages)) = json.remove("packages") {
    if let Some((npm_key, Value::Object(mut npm))) =
      packages.remove_entry("npm")
    {
      let mut pkg_had_multiple_versions: HashMap<String, bool> =
        HashMap::with_capacity(npm.len());
      for id in npm.keys() {
        let Some((name, _)) = extract_nv_from_id(id) else {
          continue; // corrupt
        };
        pkg_had_multiple_versions
          .entry(name.to_string())
          .and_modify(|v| *v = true)
          .or_default();
      }
      for value in npm.values_mut() {
        let Value::Object(value) = value else {
          continue;
        };
        let Some(Value::Object(deps)) = value.remove("dependencies") else {
          continue;
        };
        let mut new_deps = Vec::with_capacity(deps.len());
        for (key, id) in deps {
          let Value::String(id) = id else {
            continue;
          };
          let Some((name, version)) = extract_nv_from_id(&id) else {
            // corrupt
            return Err(TransformError::FailedExtractingV3NpmDepNv {
              id: id.to_string(),
            });
          };
          if name == key {
            let has_single_version = pkg_had_multiple_versions
              .get(name)
              .map(|had_multiple| !had_multiple)
              .unwrap_or(false);
            if has_single_version {
              new_deps.push(Value::String(name.to_string()));
            } else {
              new_deps.push(Value::String(format!("{}@{}", name, version)));
            }
          } else {
            new_deps
              .push(Value::String(format!("{}@npm:{}@{}", key, name, version)));
          }
        }
        value.insert("dependencies".into(), new_deps.into());
      }
      json.insert(npm_key, npm.into());
    }

    if let Some((jsr_key, Value::Object(mut jsr))) =
      packages.remove_entry("jsr")
    {
      let mut pkg_had_multiple_specifiers: HashMap<&str, bool> = HashMap::new();
      if let Some(Value::Object(specifiers)) = packages.get("specifiers") {
        pkg_had_multiple_specifiers.reserve(specifiers.len());
        for req in specifiers.keys() {
          let Some((name, _)) = split_pkg_req(req) else {
            continue; // corrupt
          };
          pkg_had_multiple_specifiers
            .entry(name)
            .and_modify(|v| *v = true)
            .or_default();
        }
      }
      for pkg in jsr.values_mut() {
        let Some(Value::Array(deps)) = pkg.get_mut("dependencies") else {
          continue;
        };
        for dep in deps.iter_mut() {
          let Value::String(dep) = dep else {
            continue;
          };
          let Some((name, _)) = split_pkg_req(dep) else {
            continue;
          };
          if let Some(false) = pkg_had_multiple_specifiers.get(name) {
            *dep = name.to_string();
          }
        }
      }
      json.insert(jsr_key, jsr.into());
    }

    if let Some(Value::Object(specifiers)) = packages.get_mut("specifiers") {
      for value in specifiers.values_mut() {
        let Value::String(value) = value else {
          continue;
        };
        let Some((_, Some(id_stripped))) = split_pkg_req(value) else {
          continue;
        };
        *value = id_stripped.to_string();
      }
    }

    // flatten packages into root
    for (key, value) in packages {
      json.insert(key, value);
    }
  }

  Ok(json)
}

pub async fn transform4_to_5<T: NpmPackageInfoProvider>(
  mut json: JsonMap,
  info_provider: &T,
) -> Result<JsonMap, TransformError> {
  json.insert("version".into(), "5".into());

  if let Some(Value::Object(mut npm)) = json.remove("npm") {
    let mut npm_packages = Vec::new();
    let mut keys = Vec::new();
    for (key, _) in &npm {
      let Some((name, version)) = extract_nv_from_id(key) else {
        continue;
      };
      let Ok(version) = Version::parse_standard(version) else {
        continue;
      };
      npm_packages.push(PackageNv {
        name: name.into(),
        version,
      });
      keys.push(key.clone());
    }
    let results = info_provider
      .get_npm_package_info(&npm_packages)
      .await
      .map_err(|source| TransformError::FailedGettingNpmPackageInfo {
        source,
      })?;
    for (key, result) in keys.iter().zip(results) {
      let Some(Value::Object(value)) = npm.get_mut(key) else {
        continue;
      };

      if let Some(Value::Array(deps)) = value.get("dependencies") {
        if deps.is_empty() {
          value.remove("dependencies");
        }
      }
      if !result.optional_dependencies.is_empty() {
        value.insert(
          "optionalDependencies".into(),
          result.optional_dependencies.into(),
        );
      }
      if !result.cpu.is_empty() {
        value.insert("cpu".into(), result.cpu.into());
      }
      if !result.os.is_empty() {
        value.insert("os".into(), result.os.into());
      }
      if let Some(tarball_url) = result.tarball_url {
        value.insert("tarball".into(), tarball_url.into());
      }
    }
    json.insert("npm".into(), npm.into());
  }

  Ok(json)
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Lockfile5NpmInfo {
  pub tarball_url: Option<String>,
  pub optional_dependencies: Vec<String>,
  pub cpu: Vec<String>,
  pub os: Vec<String>,
}

pub trait NpmPackageInfoProvider {
  fn get_npm_package_info(
    &self,
    values: &[PackageNv],
  ) -> impl Future<Output = Result<Vec<Lockfile5NpmInfo>, Box<dyn std::error::Error>>>;
}

#[cfg(test)]
mod test {
  use std::future::Future;

  use async_executor::Executor;
  use pretty_assertions::assert_eq;
  use serde_json::json;

  use super::*;

  #[test]
  fn test_transforms_1_to_2() {
    let data: JsonMap = serde_json::from_value(json!({
      "https://github.com/": "asdf",
      "https://github.com/mod.ts": "asdf2",
    }))
    .unwrap();
    let result = transform1_to_2(data);
    assert_eq!(
      result,
      serde_json::from_value(json!({
        "version": "2",
        "remote": {
          "https://github.com/": "asdf",
          "https://github.com/mod.ts": "asdf2",
        }
      }))
      .unwrap()
    );
  }

  #[test]
  fn test_transforms_2_to_3() {
    let data: JsonMap = serde_json::from_value(json!({
      "version": "2",
      "remote": {
        "https://github.com/": "asdf",
        "https://github.com/mod.ts": "asdf2",
      },
      "npm": {
        "specifiers": {
          "nanoid": "nanoid@3.3.4",
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
    })).unwrap();
    let result = transform2_to_3(data);
    assert_eq!(result, serde_json::from_value(json!({
      "version": "3",
      "remote": {
        "https://github.com/": "asdf",
        "https://github.com/mod.ts": "asdf2",
      },
      "packages": {
        "specifiers": {
          "npm:nanoid": "npm:nanoid@3.3.4",
        },
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
      }
    })).unwrap());
  }

  #[test]
  fn test_transforms_3_to_4_basic() {
    let data: JsonMap = serde_json::from_value(json!({
      "version": "3",
      "remote": {
        "https://github.com/": "asdf",
        "https://github.com/mod.ts": "asdf2",
      },
      "packages": {
        "specifiers": {
          "npm:package-a": "npm:package-a@3.3.4",
        },
        "npm": {
          "package-a@3.3.4": {
            "integrity": "sha512-MqBkQh/OHTS2egovRtLk45wEyNXwF+cokD+1YPf9u5VfJiRdAiRwB2froX5Co9Rh20xs4siNPm8naNotSD6RBw==",
            "dependencies": {
              "package-b": "package-b@1.0.0",
              "package-c": "package-c@1.0.0",
              "other": "package-d@1.0.0",
            }
          },
          "package-b@1.0.0": {
            "integrity": "sha512-foobar",
            "dependencies": {}
          },
          "package-c@1.0.0": {
            "integrity": "sha512-foobar",
            "dependencies": {}
          },
          "package-c@2.0.0": {
            "integrity": "sha512-foobar",
            "dependencies": {
              "package-e": "package-e@1.0.0_package-d@1.0.0",
            }
          },
          "package-d@1.0.0": {
            "integrity": "sha512-foobar",
            "dependencies": {}
          },
          "package-e@1.0.0_package-d@1.0.0": {
            "integrity": "sha512-foobar",
            "dependencies": {
              "package-d": "package-d@1.0.0",
            }
          }
        }
      }
    })).unwrap();
    let result = transform3_to_4(data).unwrap();
    assert_eq!(result, serde_json::from_value(json!({
      "version": "4",
      "specifiers": {
        "npm:package-a": "3.3.4",
      },
      "npm": {
        "package-a@3.3.4": {
          "integrity": "sha512-MqBkQh/OHTS2egovRtLk45wEyNXwF+cokD+1YPf9u5VfJiRdAiRwB2froX5Co9Rh20xs4siNPm8naNotSD6RBw==",
          "dependencies": [
            "other@npm:package-d@1.0.0",
            "package-b",
            "package-c@1.0.0",
          ]
        },
        "package-b@1.0.0": {
          "integrity": "sha512-foobar",
          "dependencies": []
        },
        "package-c@1.0.0": {
          "integrity": "sha512-foobar",
          "dependencies": []
        },
        "package-c@2.0.0": {
          "integrity": "sha512-foobar",
          "dependencies": [
            "package-e"
          ]
        },
        "package-d@1.0.0": {
          "integrity": "sha512-foobar",
          "dependencies": []
        },
        "package-e@1.0.0_package-d@1.0.0": {
          "integrity": "sha512-foobar",
          "dependencies": [
            "package-d"
          ]
        }
      },
      "remote": {
        "https://github.com/": "asdf",
        "https://github.com/mod.ts": "asdf2",
      },
    })).unwrap());
  }

  fn run_async<T: Send + Sync>(f: impl Future<Output = T> + Send + Sync) -> T {
    let executor = Executor::new();
    let handle = executor.run(async move { f.await });
    futures_lite::future::block_on(handle)
  }

  struct TestNpmPackageInfoProvider {
    packages: HashMap<PackageNv, Lockfile5NpmInfo>,
  }

  impl NpmPackageInfoProvider for TestNpmPackageInfoProvider {
    async fn get_npm_package_info(
      &self,
      values: &[PackageNv],
    ) -> Result<Vec<Lockfile5NpmInfo>, Box<dyn std::error::Error>> {
      Ok(
        values
          .iter()
          .map(|v| self.packages.get(v).unwrap().clone())
          .collect(),
      )
    }
  }

  fn nv(name_and_version: &str) -> PackageNv {
    PackageNv::from_str(name_and_version).unwrap()
  }

  #[test]
  fn test_transforms_4_to_5() {
    let result = run_async(async move {
      let packages = [
        (
          nv("package-a@3.3.4"),
          Lockfile5NpmInfo {
            cpu: vec!["x86_64".to_string()],
            ..Default::default()
          },
        ),
        (
          nv("package-b@1.0.0"),
          Lockfile5NpmInfo {
            cpu: vec!["x86_64".to_string()],
            ..Default::default()
          },
        ),
        (
          nv("package-c@1.0.0"),
          Lockfile5NpmInfo {
            tarball_url: Some(
              "https://registry.npmjs.org/package-c/-/package-c-1.0.0.tgz"
                .to_string(),
            ),
            optional_dependencies: vec!["opt-dep-1".to_string()],
            ..Default::default()
          },
        ),
        (
          nv("package-d@1.0.0"),
          Lockfile5NpmInfo {
            os: vec!["darwin".to_string(), "linux".to_string()],
            optional_dependencies: vec![
              "opt-dep-2".to_string(),
              "opt-dep-3".to_string(),
            ],
            ..Default::default()
          },
        ),
        (
          nv("package-e@1.0.0"),
          Lockfile5NpmInfo {
            tarball_url: Some(
              "https://registry.npmjs.org/package-e/-/package-e-1.0.0.tgz"
                .to_string(),
            ),
            cpu: vec!["arm64".to_string()],
            os: vec!["win32".to_string()],
            ..Default::default()
          },
        ),
      ];
      let data = serde_json::from_value(json!({
        "version": "4",
        "npm": {
          "package-a@3.3.4": {
            "integrity": "sha512-foobar",
            "dependencies": [
              "package-b@1.0.0",
              "package-c@1.0.0",
              "package-d@1.0.0",
              "package-e@1.0.0",
            ]
          },
          "package-b@1.0.0": {
            "integrity": "sha512-foobar",
          },
          "package-c@1.0.0": {
            "integrity": "sha512-foobar",
          },
          "package-d@1.0.0": {
            "integrity": "sha512-foobar",
          },
          "package-e@1.0.0": {
            "integrity": "sha512-foobar",
          },
        }
      }))
      .unwrap();
      transform4_to_5(
        data,
        &TestNpmPackageInfoProvider {
          packages: HashMap::from_iter(packages),
        },
      )
      .await
      .unwrap()
    });
    assert_eq!(result, serde_json::from_value(json!({
      "version": "5",
      "npm": {
        "package-a@3.3.4": {
          "cpu": ["x86_64"],
          "integrity": "sha512-foobar",
          "dependencies": [
            "package-b@1.0.0",
            "package-c@1.0.0",
            "package-d@1.0.0",
            "package-e@1.0.0",
          ],
        },
        "package-b@1.0.0": {
          "integrity": "sha512-foobar",
          "cpu": ["x86_64"],
        },
        "package-c@1.0.0": {
          "integrity": "sha512-foobar",
          "tarball": "https://registry.npmjs.org/package-c/-/package-c-1.0.0.tgz",
          "optionalDependencies": ["opt-dep-1"],
        },
        "package-d@1.0.0": {
          "integrity": "sha512-foobar",
          "os": ["darwin", "linux"],
          "optionalDependencies": ["opt-dep-2", "opt-dep-3"],
        },
        "package-e@1.0.0": {
          "integrity": "sha512-foobar",
          "tarball": "https://registry.npmjs.org/package-e/-/package-e-1.0.0.tgz",
          "cpu": ["arm64"],
          "os": ["win32"],
        },
      }
    })).unwrap());
  }
}
