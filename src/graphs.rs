// Copyright 2018-2024 the Deno authors. MIT license.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::hash::Hash;

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

impl LockfileGraphPackage {
  pub fn add_dependent(&mut self, id: LockfilePkgId) {
    match self {
      LockfileGraphPackage::Jsr(pkg) => {
        pkg.dependenents.insert(id);
      }
      LockfileGraphPackage::Npm(pkg) => {
        pkg.dependenents.insert(id);
      }
    }
  }
}

#[derive(Debug)]
struct LockfileNpmGraphPackage {
  dependenents: HashSet<LockfilePkgId>,
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

impl LockfileNpmGraphPackage {
  pub fn all_dependency_ids(
    &self,
  ) -> impl Iterator<Item = &LockfileNpmPackageId> {
    self
      .dependencies
      .values()
      .chain(self.optional_dependencies.values())
      .chain(self.optional_peers.values())
  }
}

#[derive(Debug)]
struct LockfileJsrGraphPackage {
  dependenents: HashSet<LockfilePkgId>,
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
          dependenents: HashSet::new(),
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
          dependenents: HashSet::new(),
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

    let pkg_ids = packages.keys().cloned().collect::<Vec<_>>();
    for pkg_id in pkg_ids {
      if let Some(pkg) = packages.get(&pkg_id) {
        let dependency_ids = match pkg {
          LockfileGraphPackage::Jsr(pkg) => pkg
            .dependencies
            .iter()
            .filter_map(|req| root_packages.get(req))
            .cloned()
            .collect::<Vec<_>>(),
          LockfileGraphPackage::Npm(pkg) => pkg
            .all_dependency_ids()
            .cloned()
            .map(LockfilePkgId::Npm)
            .collect::<Vec<_>>(),
        };

        for dep_id in dependency_ids {
          if let Some(pkg) = packages.get_mut(&dep_id) {
            pkg.add_dependent(pkg_id.clone());
          }
        }
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
    let mut pending_ids = package_reqs
      .map(LockfilePkgReq::from_jsr_dep)
      .filter_map(|id| self.root_packages.get(&id).cloned())
      .collect::<VecDeque<_>>();

    while let Some(pkg_id) = pending_ids.pop_front() {
      let Some(pkg) = self.packages.get_mut(&pkg_id) else {
        continue;
      };
      match pkg {
        LockfileGraphPackage::Jsr(pkg) => {
          pending_ids.extend(
            pkg
              .dependencies
              .iter()
              .filter_map(|req| self.root_packages.get(req))
              .cloned(),
          );
          pending_ids.extend(pkg.dependenents.drain());
        }
        LockfileGraphPackage::Npm(pkg) => {
          pending_ids.extend(
            pkg
              .all_dependency_ids()
              .map(|dep_id| LockfilePkgId::Npm(dep_id.clone())),
          );
          pending_ids.extend(pkg.dependenents.drain());
        }
      }
      self.remove_package(&pkg_id);
    }
  }

  fn remove_package(&mut self, id: &LockfilePkgId) {
    self.packages.remove(id);
    self.root_packages.retain(|_, pkg_id| pkg_id != id);
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
