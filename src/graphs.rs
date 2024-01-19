// Copyright 2018-2024 the Deno authors. All rights reserved. MIT license.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;

use crate::NpmPackageInfo;
use crate::PackagesContent;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum LockfilePkgId {
  Npm(LockfileNpmPackageId),
  Jsr(LockfileJsrPkgNv),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct LockfileJsrPkgNv(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct LockfileNpmPackageId(String);

impl LockfileNpmPackageId {
  pub fn parts(&self) -> impl Iterator<Item = &str> {
    let package_id = &self.0;
    let package_id = package_id.strip_prefix("npm:").unwrap_or(package_id);
    package_id.split("_").filter(|s| !s.is_empty())
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct LockfilePkgReq(String);

enum LockfileGraphPackage {
  Jsr(LockfileJsrGraphPackage),
  Npm(LockfileNpmGraphPackage),
}

struct LockfileNpmGraphPackage {
  reference_count: usize,
  integrity: String,
  dependencies: BTreeMap<String, LockfileNpmPackageId>,
}

struct LockfileJsrGraphPackage {
  reference_count: usize,
  dependencies: BTreeSet<LockfilePkgReq>,
}

pub struct LockfilePackageGraph {
  root_packages: HashMap<LockfilePkgReq, LockfilePkgId>,
  packages: HashMap<LockfilePkgId, LockfileGraphPackage>,
  removed_jsr_packages: Vec<LockfileJsrPkgNv>,
}

impl LockfilePackageGraph {
  pub fn from_lockfile<'a>(
    content: &PackagesContent,
    old_config_file_packages: impl Iterator<Item = &'a str>,
  ) -> Self {
    let mut root_packages =
      HashMap::<LockfilePkgReq, LockfilePkgId>::with_capacity(
        content.specifiers.len(),
      );
    // collect the specifiers to version mappings
    for (key, value) in &content.specifiers {
      if let Some(value) = value.strip_prefix("npm:") {
        root_packages.insert(
          LockfilePkgReq(key.to_string()),
          LockfilePkgId::Npm(LockfileNpmPackageId(value.to_string())),
        );
      } else if let Some(value) = value.strip_prefix("jsr:") {
        root_packages.insert(
          LockfilePkgReq(key.to_string()),
          LockfilePkgId::Jsr(LockfileJsrPkgNv(value.to_string())),
        );
      }
    }

    let mut packages = HashMap::new();
    for (nv, package) in &content.jsr {
      packages.insert(
        LockfilePkgId::Jsr(LockfileJsrPkgNv(nv.clone())),
        LockfileGraphPackage::Jsr(LockfileJsrGraphPackage {
          reference_count: 0,
          dependencies: package
            .dependencies
            .iter()
            .map(|req| LockfilePkgReq(req.clone()))
            .collect(),
        }),
      );
    }
    for (id, package) in &content.npm {
      packages.insert(
        LockfilePkgId::Npm(LockfileNpmPackageId(id.clone())),
        LockfileGraphPackage::Npm(LockfileNpmGraphPackage {
          reference_count: 0,
          integrity: package.integrity.clone(),
          dependencies: package
            .dependencies
            .iter()
            .map(|(key, dep_id)| {
              (key.clone(), LockfileNpmPackageId(dep_id.clone()))
            })
            .collect(),
        }),
      );
    }

    let mut visited = HashSet::new();
    let mut pending = old_config_file_packages
      .filter_map(|value| {
        content.specifiers.get(value).and_then(|value| {
          if let Some(value) = value.strip_prefix("npm:") {
            Some(LockfilePkgId::Npm(LockfileNpmPackageId(value.to_string())))
          } else if let Some(value) = value.strip_prefix("jsr:") {
            Some(LockfilePkgId::Jsr(LockfileJsrPkgNv(value.to_string())))
          } else {
            None
          }
        })
      })
      .collect::<VecDeque<_>>();
    while let Some(id) = pending.pop_back() {
      if let Some(package) = packages.get_mut(&id) {
        match package {
          LockfileGraphPackage::Jsr(package) => {
            package.reference_count += 1;
            if visited.insert(id) {
              for req in &package.dependencies {
                if let Some(nv) = root_packages.get(req) {
                  pending.push_back(nv.clone());
                }
              }
            }
          }
          LockfileGraphPackage::Npm(package) => {
            package.reference_count += 1;
            if visited.insert(id) {
              for dep_id in package.dependencies.values() {
                pending.push_back(LockfilePkgId::Npm(dep_id.clone()));
              }
            }
          }
        }
      }
    }

    Self {
      root_packages,
      packages,
      removed_jsr_packages: Default::default(),
    }
  }

  pub fn remove_root_packages<'a>(
    &mut self,
    package_reqs: impl Iterator<Item = String>,
  ) {
    let mut pending = VecDeque::new();
    let mut pending_reqs = package_reqs
      .map(|req| LockfilePkgReq(req.to_string()))
      .collect::<VecDeque<_>>();
    let mut visited_root_packages =
      HashSet::with_capacity(self.root_packages.len());
    visited_root_packages.extend(pending_reqs.iter().cloned());
    while let Some(pending_req) = pending_reqs.pop_front() {
      if let Some(id) = self.root_packages.get(&pending_req) {
        if let LockfilePkgId::Npm(id) = id {
          if let Some(first_part) = id.parts().next() {
            for (req, id) in &self.root_packages {
              if let LockfilePkgId::Npm(id) = &id {
                if id.parts().skip(1).any(|part| part == first_part) {
                  if visited_root_packages.insert(req.clone()) {
                    pending_reqs.push_back(req.clone());
                  }
                }
              }
            }
          }
        }
        pending.push_back(id.clone());
      }
    }

    while let Some(id) = pending.pop_back() {
      if let Some(package) = self.packages.get_mut(&id) {
        match package {
          LockfileGraphPackage::Jsr(package) => {
            if package.reference_count > 1 {
              package.reference_count -= 1;
            } else {
              debug_assert_eq!(package.reference_count, 1, "Package: {:?}", id);
              for req in &package.dependencies {
                if let Some(id) = self.root_packages.get(req) {
                  pending.push_back(id.clone());
                }
              }
              self.remove_package(id);
            }
          }
          LockfileGraphPackage::Npm(package) => {
            if package.reference_count > 1 {
              package.reference_count -= 1;
            } else {
              debug_assert_eq!(package.reference_count, 1, "Package: {:?}", id);
              for dep_id in package.dependencies.values() {
                pending.push_back(LockfilePkgId::Npm(dep_id.clone()));
              }
              self.remove_package(id);
            }
          }
        }
      }
    }
  }

  fn remove_package(&mut self, id: LockfilePkgId) {
    self.packages.remove(&id);
    self.root_packages.retain(|_, pkg_id| *pkg_id != id);
    if let LockfilePkgId::Jsr(nv) = id {
      self.removed_jsr_packages.push(nv);
    }
  }

  pub fn populate_packages(self, packages: &mut PackagesContent) {
    for (req, id) in self.root_packages {
      packages.specifiers.insert(
        req.0,
        match id {
          LockfilePkgId::Npm(id) => format!("npm:{}", id.0),
          LockfilePkgId::Jsr(nv) => format!("jsr:{}", nv.0),
        },
      );
    }
    for (id, package) in self.packages {
      match package {
        LockfileGraphPackage::Jsr(package) => {
          packages.jsr.insert(
            match id {
              LockfilePkgId::Jsr(nv) => nv.0,
              LockfilePkgId::Npm(_) => unreachable!(),
            },
            crate::JsrPackageInfo {
              dependencies: package
                .dependencies
                .into_iter()
                .map(|req| req.0)
                .collect(),
            },
          );
        }
        LockfileGraphPackage::Npm(package) => {
          packages.npm.insert(
            match id {
              LockfilePkgId::Jsr(_) => unreachable!(),
              LockfilePkgId::Npm(id) => id.0,
            },
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
  }

  pub fn clear_remotes_for_removed_jsr_packages(
    &self,
    redirects: &mut BTreeMap<String, String>,
    nv_to_jsr_url: impl Fn(&str) -> Option<String>,
  ) {
    for nv in &self.removed_jsr_packages {
      if let Some(url) = nv_to_jsr_url(&nv.0) {
        debug_assert!(
          url.ends_with("/"),
          "JSR URL should end with slash: {}",
          url
        );
        redirects.retain(|k, _| !k.starts_with(&url));
      }
    }
  }
}
