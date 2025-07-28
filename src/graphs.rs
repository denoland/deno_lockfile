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
    self.0.as_str().split('_').filter(|s| !s.is_empty())
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

impl LockfileGraphPackage {
  pub fn inc_reference_count(&mut self) {
    match self {
      LockfileGraphPackage::Jsr(pkg) => pkg.reference_count += 1,
      LockfileGraphPackage::Npm(pkg) => pkg.reference_count += 1,
    }
  }
}

#[derive(Debug)]
struct LockfileNpmGraphPackage {
  reference_count: usize,
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
  reference_count: usize,
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
          reference_count: 0,
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
          reference_count: 0,
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

    for (_, id) in &root_packages {
      if let Some(pkg) = packages.get_mut(id) {
        pkg.inc_reference_count();
      }
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
            .map(|id| LockfilePkgId::Npm(id))
            .collect::<Vec<_>>(),
        };

        for dep_id in dependency_ids {
          if let Some(pkg) = packages.get_mut(&dep_id) {
            pkg.inc_reference_count();
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
    let mut root_ids = Vec::new();

    // collect the root ids being removed
    {
      let mut pending_reqs = package_reqs
        .map(LockfilePkgReq::from_jsr_dep)
        .collect::<VecDeque<_>>();
      let mut visited_root_packages =
        HashSet::with_capacity(self.root_packages.len());
      visited_root_packages.extend(pending_reqs.iter().cloned());

      let id_to_root_req = self
        .root_packages
        .iter()
        .map(|(req, id)| (id, req))
        .collect::<HashMap<_, _>>();

      while let Some(pending_req) = pending_reqs.pop_front() {
        if let Some(id) = self.root_packages.get(&pending_req) {
          // add the root id to the removal queue
          root_ids.push(id.clone());
          if let Some(package) = self.packages.get(id) {
            // collect dependencies of this package
            let mut dependency_ids = HashSet::new();
            match package {
              LockfileGraphPackage::Jsr(pkg) => {
                for dep_req in &pkg.dependencies {
                  if let Some(dep_id) = self.root_packages.get(dep_req) {
                    dependency_ids.insert(dep_id.clone());
                  }
                  if visited_root_packages.insert(dep_req.clone()) {
                    pending_reqs.push_back(dep_req.clone());
                  }
                }
              }
              LockfileGraphPackage::Npm(pkg) => {
                for dep_id in pkg.dependencies.values() {
                  dependency_ids.insert(LockfilePkgId::Npm(dep_id.clone()));
                  if let Some(&root_req) =
                    id_to_root_req.get(&LockfilePkgId::Npm(dep_id.clone()))
                  {
                    if visited_root_packages.insert(root_req.clone()) {
                      pending_reqs.push_back(root_req.clone());
                    }
                  }
                }
              }
            }

            // search for other root packages that share dependencies with this package
            for (root_req, root_id) in &self.root_packages {
              if visited_root_packages.contains(root_req) {
                // already been handled
                continue;
              }

              if let Some(root_package) = self.packages.get(root_id) {
                let has_shared_dep = match root_package {
                  LockfileGraphPackage::Jsr(pkg) => {
                    pkg.dependencies.iter().any(|dep_req| {
                      self
                        .root_packages
                        .get(dep_req)
                        .map_or(false, |dep_id| dependency_ids.contains(dep_id))
                    })
                  }
                  LockfileGraphPackage::Npm(pkg) => {
                    pkg.dependencies.values().any(|dep_id| {
                      dependency_ids
                        .contains(&LockfilePkgId::Npm(dep_id.clone()))
                    })
                  }
                };

                if has_shared_dep
                  && visited_root_packages.insert(root_req.clone())
                {
                  pending_reqs.push_back(root_req.clone());
                }
              }
            }
          }

          // search for other root packages that have a peer dependency on this root package
          match id {
            LockfilePkgId::Npm(id) => {
              if let Some(first_part) = id.parts().next() {
                // when we encode package ids with peer dependencies, we replace / with +.
                // we'll be searching for the peer dependency id in the other root packages,
                // so we need to do the same replacement
                let first_part_peer_dep_id = first_part.replace("/", "+");
                for (req, id) in &self.root_packages {
                  if let LockfilePkgId::Npm(id) = &id {
                    if id
                      .parts()
                      .skip(1)
                      .any(|part| part == first_part_peer_dep_id)
                      && visited_root_packages.insert(req.clone())
                    {
                      pending_reqs.push_back(req.clone());
                    }
                  }
                }
              }
            }
            LockfilePkgId::Jsr(_) => {}
          }
        }
      }
    }

    // Go through the graph and mark the packages that no
    // longer use this root id. If the package goes to having
    // no root ids, then remove it from the graph.
    let mut seen_pkgs = HashSet::with_capacity(self.packages.len());
    while let Some(root_id) = root_ids.pop() {
      seen_pkgs.clear();
      let mut pending = VecDeque::from([root_id.clone()]);
      while let Some(id) = pending.pop_back() {
        if let Some(package) = self.packages.get_mut(&id) {
          match package {
            LockfileGraphPackage::Jsr(package) => {
              package.reference_count -= 1;
              for req in &package.dependencies {
                if let Some(id) = self.root_packages.get(req) {
                  if seen_pkgs.insert(id.clone()) {
                    pending.push_back(id.clone());
                  }
                }
              }
              if package.reference_count == 0 {
                self.remove_package(&id);
              }
            }
            LockfileGraphPackage::Npm(package) => {
              package.reference_count -= 1;
              for dep_id in package.dependencies.values() {
                let id = LockfilePkgId::Npm(dep_id.clone());
                if seen_pkgs.insert(id.clone()) {
                  pending.push_back(id);
                }
              }
              if package.reference_count == 0 {
                self.remove_package(&id);
              }
            }
          }
        }
      }
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
