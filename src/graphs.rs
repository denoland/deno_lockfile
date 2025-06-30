// Copyright 2018-2024 the Deno authors. MIT license.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;

use deno_semver::jsr::JsrDepPackageReq;
use deno_semver::package::PackageNv;
use deno_semver::package::PackageReq;
use deno_semver::SmallStackString;
use deno_semver::StackString;
use deno_semver::Version;

use crate::NpmPackageInfo;
use crate::PackagesContent;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum LockfilePkgId {
  Npm(LockfileNpmPackageId),
  Jsr(LockfileJsrPkgNv),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct LockfileJsrPkgNv(PackageNv);

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct LockfileNpmPackageId(StackString);

impl LockfileNpmPackageId {
  pub fn parts(&self) -> impl Iterator<Item = &str> {
    let package_id = self.0.as_str();
    package_id.split('_').filter(|s| !s.is_empty())
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum LockfilePkgReq {
  Jsr(PackageReq),
  Npm(PackageReq),
}

impl LockfilePkgReq {
  pub fn from_jsr_dep(dep: JsrDepPackageReq) -> Self {
    match dep.kind {
      deno_semver::package::PackageKind::Jsr => LockfilePkgReq::Jsr(dep.req),
      deno_semver::package::PackageKind::Npm => LockfilePkgReq::Npm(dep.req),
    }
  }

  pub fn into_jsr_dep(self) -> JsrDepPackageReq {
    match self {
      LockfilePkgReq::Jsr(req) => JsrDepPackageReq::jsr(req),
      LockfilePkgReq::Npm(req) => JsrDepPackageReq::npm(req),
    }
  }

  pub fn req(&self) -> &PackageReq {
    match self {
      LockfilePkgReq::Jsr(req) => req,
      LockfilePkgReq::Npm(req) => req,
    }
  }
}

#[derive(Debug)]
enum LockfileGraphPackage {
  Jsr(LockfileJsrGraphPackage),
  Npm(LockfileNpmGraphPackage),
}

#[derive(Debug)]
struct LockfileNpmGraphPackage {
  /// Root ids that transitively reference this package.
  root_ids: HashSet<LockfilePkgId>,
  integrity: Option<String>,
  dependencies: BTreeMap<StackString, LockfileNpmPackageId>,
  optional_dependencies: BTreeMap<StackString, LockfileNpmPackageId>,
  optional_peers: BTreeMap<StackString, LockfileNpmPackageId>,
  os: Vec<SmallStackString>,
  cpu: Vec<SmallStackString>,
  tarball: Option<StackString>,
  deprecated: bool,
  scripts: bool,
  bin: bool,
}

#[derive(Debug)]
struct LockfileJsrGraphPackage {
  /// Root ids that transitively reference this package.
  root_ids: HashSet<LockfilePkgId>,
  integrity: String,
  dependencies: BTreeSet<LockfilePkgReq>,
}

/// Graph used to analyze a lockfile to determine which packages
/// and remotes can be removed based on config file changes.
pub struct LockfilePackageGraph {
  root_packages: HashMap<LockfilePkgReq, LockfilePkgId>,
  packages: HashMap<LockfilePkgId, LockfileGraphPackage>,
  remotes: BTreeMap<String, String>,
}

impl LockfilePackageGraph {
  pub fn from_lockfile(
    content: PackagesContent,
    remotes: BTreeMap<String, String>,
    old_config_file_packages: impl Iterator<Item = JsrDepPackageReq>,
  ) -> Self {
    let mut root_packages =
      HashMap::<LockfilePkgReq, LockfilePkgId>::with_capacity(
        content.specifiers.len(),
      );
    // collect the specifiers to version mappings
    let package_count =
      content.specifiers.len() + content.jsr.len() + content.npm.len();
    let mut packages = HashMap::with_capacity(package_count);
    for (dep, value) in content.specifiers {
      match dep.kind {
        deno_semver::package::PackageKind::Jsr => {
          let Ok(version) = Version::parse_standard(&value) else {
            continue;
          };
          let nv = LockfilePkgId::Jsr(LockfileJsrPkgNv(PackageNv {
            name: dep.req.name.clone(),
            version,
          }));
          root_packages.insert(LockfilePkgReq::Jsr(dep.req), nv);
        }
        deno_semver::package::PackageKind::Npm => {
          let id = LockfileNpmPackageId({
            let mut text =
              StackString::with_capacity(dep.req.name.len() + 1 + value.len());
            text.push_str(&dep.req.name);
            text.push('@');
            text.push_str(&value);
            text
          });
          root_packages
            .insert(LockfilePkgReq::Npm(dep.req), LockfilePkgId::Npm(id));
        }
      }
    }

    for (nv, content_package) in content.jsr {
      packages.insert(
        LockfilePkgId::Jsr(LockfileJsrPkgNv(nv.clone())),
        LockfileGraphPackage::Jsr(LockfileJsrGraphPackage {
          root_ids: Default::default(),
          integrity: content_package.integrity.clone(),
          dependencies: content_package
            .dependencies
            .into_iter()
            .map(LockfilePkgReq::from_jsr_dep)
            .collect(),
        }),
      );
    }

    for (id, package) in content.npm {
      packages.insert(
        LockfilePkgId::Npm(LockfileNpmPackageId(id.clone())),
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
          optional_dependencies: package
            .optional_dependencies
            .iter()
            .map(|(name, dep_id)| {
              (name.clone(), LockfileNpmPackageId(dep_id.clone()))
            })
            .collect(),
          cpu: package.cpu.clone(),
          os: package.os.clone(),
          tarball: package.tarball.clone(),
          deprecated: package.deprecated,
          scripts: package.scripts,
          bin: package.bin,
          optional_peers: package
            .optional_peers
            .iter()
            .map(|(name, dep_id)| {
              (name.clone(), LockfileNpmPackageId(dep_id.clone()))
            })
            .collect(),
        }),
      );
    }

    let mut root_ids = old_config_file_packages
      .filter_map(|value| {
        root_packages
          .get(&match value.kind {
            deno_semver::package::PackageKind::Jsr => {
              LockfilePkgReq::Jsr(value.req)
            }
            deno_semver::package::PackageKind::Npm => {
              LockfilePkgReq::Npm(value.req)
            }
          })
          .cloned()
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
    }
  }

  pub fn remove_root_packages(
    &mut self,
    package_reqs: impl Iterator<Item = JsrDepPackageReq>,
  ) {
    let mut root_ids = Vec::new();

    // collect the root ids being removed
    {
      let mut pending_reqs = package_reqs
        .map(LockfilePkgReq::from_jsr_dep)
        .collect::<VecDeque<_>>();
      let mut visited_root_packages =
        HashSet::with_capacity(self.root_packages.len());
      visited_root_packages.extend(pending_reqs.iter().cloned());
      while let Some(pending_req) = pending_reqs.pop_front() {
        if let Some(id) = self.root_packages.get(&pending_req) {
          if let LockfilePkgId::Npm(id) = id {
            if let Some(first_part) = id.parts().next() {
              let first_part = first_part.replace("/", "+");
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
  }

  pub fn populate_packages(
    self,
    packages: &mut PackagesContent,
    remotes: &mut BTreeMap<String, String>,
  ) {
    *remotes = self.remotes;
    for (req, id) in self.root_packages {
      let value = match &id {
        LockfilePkgId::Jsr(nv) => {
          nv.0.version.to_custom_string::<SmallStackString>()
        }
        LockfilePkgId::Npm(id) => id
          .0
          .as_str()
          .strip_prefix(req.req().name.as_str())
          .unwrap()
          .strip_prefix("@")
          .unwrap()
          .into(),
      };
      packages.specifiers.insert(req.into_jsr_dep(), value);
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
              integrity: package.integrity,
              dependencies: package
                .dependencies
                .into_iter()
                .map(|req| req.into_jsr_dep())
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
              integrity: package.integrity,
              dependencies: package
                .dependencies
                .into_iter()
                .map(|(name, id)| (name, id.0))
                .collect(),
              cpu: package.cpu,
              os: package.os,
              tarball: package.tarball.clone(),
              optional_dependencies: package
                .optional_dependencies
                .into_iter()
                .map(|(name, id)| (name, id.0))
                .collect(),
              deprecated: package.deprecated,
              scripts: package.scripts,
              bin: package.bin,
              optional_peers: package
                .optional_peers
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
