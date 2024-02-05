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
    package_id.split('_').filter(|s| !s.is_empty())
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct LockfilePkgReq(String);

enum LockfileGraphPackage {
  Jsr(LockfileJsrGraphPackage),
  Npm(LockfileNpmGraphPackage),
}

struct LockfileNpmGraphPackage {
  /// Root ids that transitively reference this package.
  root_ids: HashSet<LockfilePkgId>,
  integrity: String,
  dependencies: BTreeMap<String, LockfileNpmPackageId>,
}

#[derive(Default)]
struct LockfileJsrGraphPackage {
  /// Root ids that transitively reference this package.
  root_ids: HashSet<LockfilePkgId>,
  dependencies: BTreeSet<LockfilePkgReq>,
}

/// Graph used to analyze a lockfile to determine which packages
/// and remotes can be removed based on config file changes.
pub struct LockfilePackageGraph<FNvToJsrUrl: Fn(&str) -> Option<String>> {
  root_packages: HashMap<LockfilePkgReq, LockfilePkgId>,
  packages: HashMap<LockfilePkgId, LockfileGraphPackage>,
  remotes: BTreeMap<String, String>,
  nv_to_jsr_url: FNvToJsrUrl,
}

impl<FNvToJsrUrl: Fn(&str) -> Option<String>>
  LockfilePackageGraph<FNvToJsrUrl>
{
  pub fn from_lockfile<'a>(
    content: PackagesContent,
    remotes: BTreeMap<String, String>,
    old_config_file_packages: impl Iterator<Item = &'a str>,
    nv_to_jsr_url: FNvToJsrUrl,
  ) -> Self {
    let mut root_packages =
      HashMap::<LockfilePkgReq, LockfilePkgId>::with_capacity(
        content.specifiers.len(),
      );
    // collect the specifiers to version mappings
    let package_count =
      content.specifiers.len() + content.jsr.len() + content.npm.len();
    let mut packages = HashMap::with_capacity(package_count);
    for (key, value) in content.specifiers {
      if let Some(value) = value.strip_prefix("npm:") {
        root_packages.insert(
          LockfilePkgReq(key.to_string()),
          LockfilePkgId::Npm(LockfileNpmPackageId(value.to_string())),
        );
      } else if let Some(value) = value.strip_prefix("jsr:") {
        let nv = LockfilePkgId::Jsr(LockfileJsrPkgNv(value.to_string()));
        root_packages.insert(LockfilePkgReq(key), nv.clone());
        packages.insert(
          nv,
          LockfileGraphPackage::Jsr(LockfileJsrGraphPackage::default()),
        );
      }
    }

    for (nv, content_package) in content.jsr {
      let id = LockfilePkgId::Jsr(LockfileJsrPkgNv(nv.clone()));
      let new_deps = &content_package.dependencies;
      let package = packages.entry(id).or_insert_with(|| {
        LockfileGraphPackage::Jsr(LockfileJsrGraphPackage::default())
      });
      match package {
        LockfileGraphPackage::Jsr(package) => {
          package.dependencies = new_deps
            .iter()
            .map(|req| LockfilePkgReq(req.clone()))
            .collect();
        }
        LockfileGraphPackage::Npm(_) => unreachable!(),
      }
    }
    for (id, package) in content.npm {
      let id = LockfilePkgId::Npm(LockfileNpmPackageId(id.clone()));
      packages.insert(
        id,
        LockfileGraphPackage::Npm(LockfileNpmGraphPackage {
          root_ids: Default::default(),
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

    let mut root_ids = old_config_file_packages
      .filter_map(|value| {
        let req = LockfilePkgReq(value.to_string());
        root_packages.get(&req).cloned()
      })
      .collect::<Vec<_>>();
    let mut unseen_root_pkg_ids =
      root_packages.values().collect::<HashSet<_>>();

    // trace every root identifier through the graph finding all corresponding packages
    while let Some(root_id) = root_ids.pop() {
      let mut pending = VecDeque::with_capacity(package_count);
      pending.push_back(root_id.clone());
      while let Some(id) = pending.pop_back() {
        unseen_root_pkg_ids.remove(&id);
        if let Some(package) = packages.get_mut(&id) {
          match package {
            LockfileGraphPackage::Jsr(package) => {
              if package.root_ids.insert(root_id.clone()) {
                for req in &package.dependencies {
                  if let Some(nv) = root_packages.get(req) {
                    pending.push_back(nv.clone());
                  }
                }
              }
            }
            LockfileGraphPackage::Npm(package) => {
              if package.root_ids.insert(root_id.clone()) {
                for dep_id in package.dependencies.values() {
                  pending.push_back(LockfilePkgId::Npm(dep_id.clone()));
                }
              }
            }
          }
        }
      }

      if root_ids.is_empty() {
        // Certain root package specifiers might not be referenced or transitively
        // referenced in the config file. For those cases, keep them in the config file.
        root_ids.extend(unseen_root_pkg_ids.drain().cloned());
      }
    }

    Self {
      root_packages,
      packages,
      remotes,
      nv_to_jsr_url,
    }
  }

  pub fn remove_root_packages(
    &mut self,
    package_reqs: impl Iterator<Item = String>,
  ) {
    let mut root_ids = Vec::new();

    // collect the root ids being removed
    {
      let mut pending_reqs =
        package_reqs.map(LockfilePkgReq).collect::<VecDeque<_>>();
      let mut visited_root_packages =
        HashSet::with_capacity(self.root_packages.len());
      visited_root_packages.extend(pending_reqs.iter().cloned());
      while let Some(pending_req) = pending_reqs.pop_front() {
        if let Some(id) = self.root_packages.get(&pending_req) {
          if let LockfilePkgId::Npm(id) = id {
            if let Some(first_part) = id.parts().next() {
              for (req, id) in &self.root_packages {
                if let LockfilePkgId::Npm(id) = &id {
                  // be a bit aggressive and remove any npm packages that
                  // have this package as a peer dependency
                  if id.parts().skip(1).any(|part| part == first_part) {
                    let has_visited = visited_root_packages.insert(req.clone());
                    if has_visited {
                      pending_reqs.push_back(req.clone());
                    }
                  }
                }
              }
            }
          }
          root_ids.push(id.clone());
        }
      }
    }

    // Go through the graph and mark the packages that no
    // longer use this root id. If the package goes to having
    // no root ids, then remove it from the graph.
    while let Some(root_id) = root_ids.pop() {
      let mut pending = VecDeque::from([root_id.clone()]);
      while let Some(id) = pending.pop_back() {
        if let Some(package) = self.packages.get_mut(&id) {
          match package {
            LockfileGraphPackage::Jsr(package) => {
              if package.root_ids.remove(&root_id) {
                for req in &package.dependencies {
                  if let Some(id) = self.root_packages.get(req) {
                    pending.push_back(id.clone());
                  }
                }
                if package.root_ids.is_empty() {
                  self.remove_package(id);
                }
              }
            }
            LockfileGraphPackage::Npm(package) => {
              if package.root_ids.remove(&root_id) {
                for dep_id in package.dependencies.values() {
                  pending.push_back(LockfilePkgId::Npm(dep_id.clone()));
                }
                if package.root_ids.is_empty() {
                  self.remove_package(id);
                }
              }
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
      if let Some(url) = (self.nv_to_jsr_url)(&nv.0) {
        debug_assert!(
          url.ends_with('/'),
          "JSR URL should end with slash: {}",
          url
        );
        self.remotes.retain(|k, _| !k.starts_with(&url));
      }
    }
  }

  pub fn populate_packages(
    self,
    packages: &mut PackagesContent,
    remotes: &mut BTreeMap<String, String>,
  ) {
    *remotes = self.remotes;
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
          if !package.dependencies.is_empty() {
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
}
