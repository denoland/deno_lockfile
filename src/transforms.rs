// Copyright 2018-2024 the Deno authors. MIT license.

use std::collections::HashMap;

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
}

pub fn transform3_to_4(mut json: JsonMap) -> Result<JsonMap, TransformError> {
  // note: although these functions are found elsewhere in this repo,
  // it is purposefully duplicated here to ensure it never changes
  // for this transform
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

    // flatten packages into root
    for (key, value) in packages {
      json.insert(key, value);
    }
  }

  Ok(json)
}

#[cfg(test)]
mod test {
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
        "npm:package-a": "npm:package-a@3.3.4",
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
}
