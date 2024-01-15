// Copyright 2018-2024 the Deno authors. All rights reserved. MIT license.

pub type JsonMap = serde_json::Map<String, serde_json::Value>;

pub fn transform1_to_2(json: JsonMap) -> JsonMap {
  let mut new_map = JsonMap::new();
  new_map.insert("version".to_string(), "2".into());
  new_map.insert("remote".to_string(), json.into());
  new_map
}

pub fn transform2_to_3(mut json: JsonMap) -> JsonMap {
  json.insert("version".into(), "3".into());
  if let Some(serde_json::Value::Object(mut npm_obj)) = json.remove("npm") {
    let mut new_obj = JsonMap::new();
    if let Some(packages) = npm_obj.remove("packages") {
      new_obj.insert("npm".into(), packages);
    }
    if let Some(serde_json::Value::Object(specifiers)) =
      npm_obj.remove("specifiers")
    {
      let mut new_specifiers = JsonMap::new();
      for (key, value) in specifiers {
        if let serde_json::Value::String(value) = value {
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
}
