// Copyright 2018-2024 the Deno authors. All rights reserved. MIT license.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;

use crate::NpmPackageInfo;
use crate::PackagesContent;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct LockfileNpmPackageId(String);

struct LockfileNpmGraphPackage {
  reference_count: usize,
  integrity: String,
  dependencies: BTreeMap<String, LockfileNpmPackageId>,
}

pub struct LockfileNpmGraph {
  root_packages: HashMap<String, LockfileNpmPackageId>,
  packages: HashMap<LockfileNpmPackageId, LockfileNpmGraphPackage>,
}

impl LockfileNpmGraph {
  pub fn from_lockfile(content: &PackagesContent) -> Self {
    let mut root_packages =
      HashMap::<String, LockfileNpmPackageId>::with_capacity(
        content.specifiers.len(),
      );
    // collect the specifiers to version mappings
    for (key, value) in &content.specifiers {
      if let Some(key) = key.strip_prefix("npm:") {
        if let Some(value) = value.strip_prefix("npm:") {
          root_packages
            .insert(key.to_string(), LockfileNpmPackageId(value.to_string()));
        }
      }
    }

    let mut packages = HashMap::new();
    for (id, package) in &content.npm {
      packages.insert(
        LockfileNpmPackageId(id.clone()),
        LockfileNpmGraphPackage {
          reference_count: 0,
          integrity: package.integrity.clone(),
          dependencies: package
            .dependencies
            .iter()
            .map(|(key, dep_id)| {
              (key.clone(), LockfileNpmPackageId(dep_id.clone()))
            })
            .collect(),
        },
      );
    }

    let mut visited = HashSet::new();
    let mut pending = root_packages.values().cloned().collect::<VecDeque<_>>();
    while let Some(id) = pending.pop_back() {
      if let Some(package) = packages.get_mut(&id) {
        package.reference_count += 1;
        if visited.insert(id) {
          for dep_id in package.dependencies.values() {
            pending.push_back(dep_id.clone());
          }
        }
      }
    }

    Self {
      root_packages,
      packages,
    }
  }

  pub fn remove_root_packages<'a>(
    &mut self,
    package_reqs: impl Iterator<Item = &'a str>,
  ) {
    fn package_id_parts(
      package_id: &LockfileNpmPackageId,
    ) -> impl Iterator<Item = &str> {
      let package_id = &package_id.0;
      let package_id = package_id.strip_prefix("npm:").unwrap_or(package_id);
      package_id.split("_").filter(|s| !s.is_empty())
    }

    let mut pending = VecDeque::new();
    let mut pending_reqs = package_reqs
      .map(|req| Cow::Borrowed(req))
      .collect::<VecDeque<_>>();
    while let Some(pending_req) = pending_reqs.pop_front() {
      if let Some(package_id) = self.root_packages.remove(pending_req.as_ref())
      {
        if let Some(first_part) = package_id_parts(&package_id).next() {
          for (req, id) in &self.root_packages {
            if package_id_parts(id).any(|part| part == first_part) {
              pending_reqs.push_back(Cow::Owned(req.clone()));
            }
          }
        }
        pending.push_back(package_id);
      }
    }

    while let Some(id) = pending.pop_back() {
      eprintln!("HANDLING: {}", id.0);
      if let Some(package) = self.packages.get_mut(&id) {
        package.reference_count -= 1;
        if package.reference_count == 0 {
          for dep_id in package.dependencies.values() {
            pending.push_back(dep_id.clone());
          }
          self.packages.remove(&id);
        }
      }
    }
  }

  pub fn populate_packages(self, packages: &mut PackagesContent) {
    for (req, id) in self.root_packages {
      eprintln!("ADDING: {:#?}", req);
      packages
        .specifiers
        .insert(format!("npm:{}", req), format!("npm:{}", id.0));
    }
    for (id, package) in self.packages {
      eprintln!("ADDING: {}", id.0);
      packages.npm.insert(
        id.0,
        NpmPackageInfo {
          integrity: package.integrity.clone(),
          dependencies: package
            .dependencies
            .into_iter()
            .map(|(name, id)| (name, id.0))
            .collect(),
        },
      );
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct LockfileJsrNv(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct LockfileJsrReq(String);

struct LockfileJsrGraphPackage {
  reference_count: usize,
  dependencies: BTreeSet<LockfileJsrReq>,
}

pub struct LockfileJsrGraph {
  root_packages: HashMap<LockfileJsrReq, LockfileJsrNv>,
  packages: HashMap<LockfileJsrNv, LockfileJsrGraphPackage>,
}

impl LockfileJsrGraph {
  pub fn from_lockfile(content: &PackagesContent) -> Self {
    let mut root_packages =
      HashMap::<LockfileJsrReq, LockfileJsrNv>::with_capacity(
        content.specifiers.len(),
      );
    // collect the specifiers to version mappings
    for (key, value) in &content.specifiers {
      if let Some(key) = key.strip_prefix("jsr:") {
        if let Some(value) = value.strip_prefix("jsr:") {
          root_packages.insert(
            LockfileJsrReq(key.to_string()),
            LockfileJsrNv(value.to_string()),
          );
        }
      }
    }

    let mut packages = HashMap::new();
    for (nv, package) in &content.jsr {
      packages.insert(
        LockfileJsrNv(nv.clone()),
        LockfileJsrGraphPackage {
          reference_count: 0,
          dependencies: package
            .dependencies
            .iter()
            .map(|req| LockfileJsrReq(req.clone()))
            .collect(),
        },
      );
    }

    let mut visited = HashSet::new();
    let mut pending = root_packages.values().cloned().collect::<VecDeque<_>>();
    while let Some(nv) = pending.pop_back() {
      if let Some(package) = packages.get_mut(&nv) {
        package.reference_count += 1;
        if visited.insert(nv) {
          for req in &package.dependencies {
            if let Some(nv) = root_packages.get(req) {
              pending.push_back(nv.clone());
            }
          }
        }
      }
    }

    Self {
      root_packages,
      packages,
    }
  }

  pub fn remove_root_packages<'a>(
    &mut self,
    package_reqs: impl Iterator<Item = &'a str>,
  ) {
    let mut pending = VecDeque::new();
    for package_req in package_reqs {
      eprintln!("REMOVING package req: {}", package_req);
      if let Some(package_nv) = self
        .root_packages
        .remove(&LockfileJsrReq(package_req.to_string()))
      {
        pending.push_back(package_nv);
      }
    }

    while let Some(nv) = pending.pop_back() {
      if let Some(package) = self.packages.get_mut(&nv) {
        package.reference_count -= 1;
        if package.reference_count == 0 {
          for req in &package.dependencies {
            if let Some(nv) = self.root_packages.get(req) {
              pending.push_back(nv.clone());
            }
          }
          self.packages.remove(&nv);
          if let Some((req, _)) =
            self.root_packages.iter().find(|(_, pkg_nv)| **pkg_nv == nv)
          {
            self.root_packages.remove(&req.clone());
          }
        }
      }
    }
  }

  pub fn populate_packages(self, packages: &mut PackagesContent) {
    for (req, nv) in self.root_packages {
      eprintln!("ADDING: {:#?}", req.0);
      packages
        .specifiers
        .insert(format!("jsr:{}", req.0), format!("jsr:{}", nv.0));
    }
    for (nv, package) in self.packages {
      eprintln!("ADDING: {}", nv.0);
      packages.jsr.insert(
        nv.0,
        crate::JsrPackageInfo {
          dependencies: package
            .dependencies
            .into_iter()
            .map(|req| req.0)
            .collect(),
        },
      );
    }
  }
}
