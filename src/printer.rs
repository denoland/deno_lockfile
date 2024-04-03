// Copyright 2018-2024 the Deno authors. All rights reserved. MIT license.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fmt::Write;

use crate::JsrPackageInfo;
use crate::LockfileContent;
use crate::NpmPackageInfo;
use crate::WorkspaceConfigContent;
use crate::WorkspaceMemberConfigContent;

struct Writer<'a, TWrite: Write> {
  inner: &'a mut TWrite,
}

impl<'a, TWrite: Write> Writer<'a, TWrite> {
  fn new(inner: &'a mut TWrite) -> Self {
    Self { inner }
  }

  fn write(&mut self, s: &str) {
    self.inner.write_str(s).unwrap();
  }
}

pub fn print_lockfile_content<TWrite: Write>(
  content: &LockfileContent,
  writer: &mut TWrite,
) {
  let mut writer = Writer::new(writer);

  // this attempts to be heavily optimized for performance and thus hardcodes indentation
  writer.write("{\n  \"version\": \"4\",\n");

  // order these based on auditability
  let packages = &content.packages;
  if !packages.specifiers.is_empty() {
    writer.write("  \"specifiers\": {\n");
    for (key, value) in &packages.specifiers {
      writer.write("    \"");
      // todo: json escape?
      writer.write(key);
      writer.write("\": \"");
      // todo: json escape?
      writer.write(value);
      writer.write(",\n");
    }
    writer.write("  },\n");
  }
  if !packages.jsr.is_empty() {
    write_jsr(&mut writer, &packages.jsr);
  }
  if !packages.npm.is_empty() {
    write_npm(&mut writer, &packages.npm);
  }
  if !content.redirects.is_empty() {
    write_redirects(&mut writer, &content.redirects);
  }
  if !content.remote.is_empty() {
    write_remote(&mut writer, &content.remote);
  }
  if !content.workspace.is_empty() {
    write_workspace(&mut writer, &content.workspace);
  }
  writer.write("\n}\n");
}

fn write_jsr(
  writer: &mut Writer<impl Write>,
  jsr: &BTreeMap<String, JsrPackageInfo>,
) {
  writer.write("  \"jsr\": {\n");
  for (key, value) in jsr {
    writer.write("    \"");
    writer.write(key);
    writer.write("\": {\n");
    writer.write("      \"integrity\": \"");
    writer.write(&value.integrity);
    writer.write("\",\n");
    if !value.dependencies.is_empty() {
      writer.write("      \"dependencies\": [\n");
      for dep in &value.dependencies {
        writer.write("        \"");
        writer.write(dep);
        writer.write("\",\n");
      }
      writer.write("      ],\n");
    }
    writer.write("    },\n");
  }
  writer.write("  },\n");
}

fn write_npm(
  writer: &mut Writer<impl Write>,
  npm: &BTreeMap<String, NpmPackageInfo>,
) {
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

  writer.write("  \"npm\": {\n");
  for (key, value) in npm {
    writer.write("    \"");
    writer.write(key);
    writer.write("\": {\n");
    writer.write("      \"integrity\": \"");
    writer.write(&value.integrity);
    writer.write("\",");
    if !value.dependencies.is_empty() {
      writer.write("      \"dependencies\": [\n");
      for (key, id) in &value.dependencies {
        let (name, version) = extract_nv_from_id(id).unwrap();
        writer.write("        \"");
        if name == key {
          let has_single_version = pkg_had_multiple_versions
            .get(name)
            .map(|had_multiple| !had_multiple)
            .unwrap_or(false);
          if has_single_version {
            writer.write(name);
          } else {
            writer.write(name);
            writer.write("@");
            writer.write(version);
          }
        } else {
          writer.write(key);
          writer.write("@npm:");
          writer.write(name);
          writer.write("@");
          writer.write(version);
        }
        writer.write("\",\n");
      }
      writer.write("      ],\n");
    }
    writer.write("    },\n");
  }
  writer.write("  },\n");
}

fn write_redirects(
  writer: &mut Writer<impl Write>,
  redirects: &BTreeMap<String, String>,
) {
  writer.write("  \"redirects\": {\n");
  for (key, value) in redirects {
    writer.write("    \"");
    writer.write(key);
    writer.write("\": \"");
    writer.write(value);
    writer.write("\",\n");
  }
  writer.write("  },\n");
}

fn write_remote(
  writer: &mut Writer<impl Write>,
  remote: &BTreeMap<String, String>,
) {
  writer.write("  \"remote\": {\n");
  for (key, value) in remote {
    writer.write("    \"");
    writer.write(key);
    writer.write("\": \"");
    writer.write(value);
    writer.write("\",\n");
  }
  writer.write("  },\n");
}

fn write_workspace(
  writer: &mut Writer<impl Write>,
  workspace: &WorkspaceConfigContent,
) {
  writer.write("  \"workspace\": {\n");
  write_workspace_member_config(writer, &workspace.root, "    ");
  if !workspace.members.is_empty() {
    writer.write("    \"members\": {\n");
    for (key, value) in &workspace.members {
      writer.write("      \"");
      writer.write(key);
      writer.write("\": {\n");
      write_workspace_member_config(writer, value, "        ");
      writer.write("      },\n");
    }
  }
  writer.write("    },\n");
  writer.write("  },\n");
}

fn write_workspace_member_config(
  writer: &mut Writer<impl Write>,
  root: &WorkspaceMemberConfigContent,
  indent_text: &str,
) {
  writer.write(indent_text);
  writer.write("\"dependencies\": [\n");
  for dep in &root.dependencies {
    writer.write(indent_text);
    writer.write("  \"");
    writer.write(dep);
    writer.write("\",\n");
  }
  writer.write(indent_text);
  writer.write("  ],\n");
  writer.write(indent_text);
  writer.write("  \"packageJson\": {\n");
  writer.write(indent_text);
  writer.write("    \"dependencies\": [\n");
  for dep in &root.package_json.dependencies {
    writer.write(indent_text);
    writer.write("      \"");
    writer.write(dep);
    writer.write("\",\n");
  }
  writer.write(indent_text);
  writer.write("],\n");
}
