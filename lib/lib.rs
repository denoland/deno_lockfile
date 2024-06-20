// Copyright 2024 the Deno authors. MIT license.

use std::path::PathBuf;

use deno_lockfile::Lockfile;
use deno_lockfile::NpmPackageDependencyLockfileInfo;
use deno_lockfile::NpmPackageInfo;
use deno_lockfile::NpmPackageLockfileInfo;
use deno_lockfile::SetWorkspaceConfigOptions;
use deno_lockfile::WorkspaceConfig;

use serde::Serialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct JsLockfile(Lockfile);

#[wasm_bindgen]
impl JsLockfile {
  #[wasm_bindgen(constructor)]
  pub fn new(filename: String, overwrite: bool) -> Self {
    JsLockfile(Lockfile::new_empty(PathBuf::from(filename), overwrite))
  }

  #[wasm_bindgen(js_name = filename)]
  pub fn filename(&self) -> String {
    self.0.filename.display().to_string()
  }

  #[wasm_bindgen(js_name = copy)]
  pub fn copy(&self) -> JsLockfile {
    JsLockfile(self.0.clone())
  }

  #[wasm_bindgen(js_name = toString)]
  pub fn to_string(&self) -> String {
    self.0.as_json_string()
  }

  #[wasm_bindgen(js_name = toJson)]
  pub fn to_json(&self) -> JsValue {
    let serializer =
      serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
    self.0.content.serialize(&serializer).unwrap()
  }

  #[wasm_bindgen(js_name = setWorkspaceConfig)]
  pub fn set_workspace_config(&mut self, config: JsValue) {
    let config: WorkspaceConfig =
      serde_wasm_bindgen::from_value(config).unwrap();
    self.0.set_workspace_config(SetWorkspaceConfigOptions {
      config,
      no_config: false,
      no_npm: false,
    });
  }

  #[wasm_bindgen(js_name = insertRemote)]
  pub fn insert_remote(&mut self, specifier: String, hash: String) {
    self.0.insert_remote(specifier, hash);
  }

  #[wasm_bindgen(js_name = insertNpmPackage)]
  pub fn insert_npm_package(
    &mut self,
    specifier: String,
    package_info: JsValue,
  ) {
    let package_info: NpmPackageInfo =
      serde_wasm_bindgen::from_value(package_info).unwrap();

    let dependencies = package_info
      .dependencies
      .into_iter()
      .map(|(k, v)| NpmPackageDependencyLockfileInfo { name: k, id: v })
      .collect();

    self.0.insert_npm_package(NpmPackageLockfileInfo {
      serialized_id: specifier,
      integrity: package_info.integrity,
      dependencies,
    });
  }

  #[wasm_bindgen(js_name = insertPackageSpecifier)]
  pub fn insert_package_specifier(
    &mut self,
    requirement: String,
    identifier: String,
  ) {
    self.0.insert_package_specifier(requirement, identifier);
  }

  #[wasm_bindgen(js_name = insertPackage)]
  pub fn insert_package(&mut self, specifier: String, integrity: String) {
    self.0.insert_package(specifier, integrity);
  }

  #[wasm_bindgen(js_name = addPackageDeps)]
  pub fn add_package_deps(
    &mut self,
    specifier: &str,
    dependencies: Vec<String>,
  ) {
    self.0.add_package_deps(specifier, dependencies.into_iter());
  }

  #[wasm_bindgen(js_name = insertRedirect)]
  pub fn insert_redirect(&mut self, from: String, to: String) {
    self.0.insert_redirect(from, to);
  }
}

#[wasm_bindgen(js_name = parseFromJson)]
pub fn js_parse_from_json(
  filename: String,
  content: &str,
) -> Result<JsLockfile, JsError> {
  Lockfile::with_lockfile_content(PathBuf::from(filename), content, false)
    .map(|lockfile| JsLockfile(lockfile))
    .map_err(|err| JsError::new(&err.to_string()))
}
