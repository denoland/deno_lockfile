// Copyright 2018-2024 the Deno authors. All rights reserved. MIT license.

use std::collections::BTreeMap;
use std::collections::HashMap;

use crate::JsrPackageInfo;
use crate::LockfileContent;
use crate::NpmPackageInfo;
use crate::WorkspaceConfigContent;
use crate::WorkspaceMemberConfigContent;

// Note: This is temporary until we upgrade to v4

// todo: investigate json escaping, which might not be necessary here
pub fn print_v4_content(content: &LockfileContent, output: &mut String) {
  // this attempts to be heavily optimized for performance and thus hardcodes indentation
  output.push_str("{\n  \"version\": \"4\"");

  // order these based on auditability
  let packages = &content.packages;
  if !packages.specifiers.is_empty() {
    output.push_str(",\n  \"specifiers\": {\n");
    for (i, (key, value)) in packages.specifiers.iter().enumerate() {
      if i > 0 {
        output.push_str(",\n");
      }
      output.push_str("    \"");
      output.push_str(key);
      output.push_str("\": \"");
      output.push_str(value);
      output.push_str("\"");
    }
    output.push_str("\n  }");
  }

  if !packages.jsr.is_empty() {
    write_jsr(output, &packages.jsr);
  }
  if !packages.npm.is_empty() {
    write_npm(output, &packages.npm);
  }
  if !content.redirects.is_empty() {
    write_redirects(output, &content.redirects);
  }
  if !content.remote.is_empty() {
    write_remote(output, &content.remote);
  }
  if !content.workspace.is_empty() {
    write_workspace(output, &content.workspace);
  }
  output.push_str("\n}\n");
}

fn write_jsr(output: &mut String, jsr: &BTreeMap<String, JsrPackageInfo>) {
  output.push_str(",\n  \"jsr\": {\n");
  for (i, (key, value)) in jsr.iter().enumerate() {
    if i > 0 {
      output.push_str(",\n");
    }
    output.push_str("    \"");
    output.push_str(key);
    output.push_str("\": {\n");
    output.push_str("      \"integrity\": \"");
    output.push_str(&value.integrity);
    output.push_str("\"");
    if !value.dependencies.is_empty() {
      output.push_str(",\n      \"dependencies\": [\n");
      for (i, dep) in value.dependencies.iter().enumerate() {
        if i > 0 {
          output.push_str(",\n");
        }
        output.push_str("        \"");
        output.push_str(dep);
        output.push_str("\"");
      }
      output.push_str("\n      ]");
    }
    output.push_str("\n    }");
  }
  output.push_str("\n  }");
}

fn write_npm(output: &mut String, npm: &BTreeMap<String, NpmPackageInfo>) {
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
    let Some((name, _)) = extract_nv_from_id(&id) else {
      continue; // corrupt
    };
    pkg_had_multiple_versions
      .entry(name)
      .and_modify(|v| *v = true)
      .or_default();
  }

  output.push_str(",\n  \"npm\": {\n");
  for (i, (key, value)) in npm.iter().enumerate() {
    if i > 0 {
      output.push_str(",\n");
    }
    output.push_str("    \"");
    output.push_str(key);
    output.push_str("\": {\n");
    output.push_str("      \"integrity\": \"");
    output.push_str(&value.integrity);
    output.push('"');
    if !value.dependencies.is_empty() {
      output.push_str(",\n      \"dependencies\": [\n");
      for (i, (key, id)) in value.dependencies.iter().enumerate() {
        if i > 0 {
          output.push_str(",\n");
        }
        let (name, version) = extract_nv_from_id(id).unwrap();
        output.push_str("        \"");
        if name == key {
          let has_single_version = pkg_had_multiple_versions
            .get(name)
            .map(|had_multiple| !had_multiple)
            .unwrap_or(false);
          if has_single_version {
            output.push_str(name);
          } else {
            output.push_str(name);
            output.push('@');
            output.push_str(version);
          }
        } else {
          output.push_str(key);
          output.push_str("@npm:");
          output.push_str(name);
          output.push('@');
          output.push_str(version);
        }
        output.push_str("\"");
      }
      output.push_str("\n      ]");
    }
    output.push_str("\n    }");
  }
  output.push_str("\n  }");
}

fn write_redirects(output: &mut String, redirects: &BTreeMap<String, String>) {
  output.push_str(",\n  \"redirects\": {\n");
  for (i, (key, value)) in redirects.iter().enumerate() {
    if i > 0 {
      output.push_str(",\n");
    }
    output.push_str("    \"");
    output.push_str(key);
    output.push_str("\": \"");
    output.push_str(value);
    output.push('\"');
  }
  output.push_str("\n  }");
}

fn write_remote(output: &mut String, remote: &BTreeMap<String, String>) {
  output.push_str(",\n  \"remote\": {\n");
  for (i, (key, value)) in remote.iter().enumerate() {
    if i > 0 {
      output.push_str(",\n");
    }
    output.push_str("    \"");
    output.push_str(key);
    output.push_str("\": \"");
    output.push_str(value);
    output.push('\"');
  }
  output.push_str("\n  }");
}

fn write_workspace(output: &mut String, workspace: &WorkspaceConfigContent) {
  output.push_str(",\n  \"workspace\": {\n");
  write_workspace_member_config(output, &workspace.root, "    ");
  if !workspace.members.is_empty() {
    output.push_str("    \"members\": {\n");
    for (i, (key, value)) in workspace.members.iter().enumerate() {
      if i > 0 {
        output.push_str(",\n");
      }
      output.push_str("      \"");
      output.push_str(key);
      output.push_str("\": {\n");
      write_workspace_member_config(output, value, "        ");
      output.push_str("\n      }");
    }
  }
  output.push_str("\n    }\n  }");
}

fn write_workspace_member_config(
  output: &mut String,
  root: &WorkspaceMemberConfigContent,
  indent_text: &str,
) {
  output.push_str(indent_text);
  output.push_str("\"dependencies\": [\n");
  for (i, dep) in root.dependencies.iter().enumerate() {
    if i > 0 {
      output.push_str(",\n");
    }
    output.push_str(indent_text);
    output.push_str("  \"");
    output.push_str(dep);
    output.push_str("\"");
  }
  output.push('\n');
  output.push_str(indent_text);
  output.push_str("],\n");
  output.push_str(indent_text);
  output.push_str("\"packageJson\": {\n");
  output.push_str(indent_text);
  output.push_str("  \"dependencies\": [\n");
  for (i, dep) in root.package_json.dependencies.iter().enumerate() {
    if i > 0 {
      output.push_str(",\n");
    }
    output.push_str(indent_text);
    output.push_str("    \"");
    output.push_str(dep);
    output.push_str("\"");
  }
  output.push('\n');
  output.push_str(indent_text);
  output.push_str("  ]");
}
