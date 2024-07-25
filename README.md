# `deno_lockfile`

[![](https://img.shields.io/crates/v/deno_lockfile.svg)](https://crates.io/crates/deno_lockfile)

This crate implements the lockfile format used by Deno.

## Format

```typescript
// TODO: Make sure all objects and arrays are sorted alphabetically

type SpecificVersion = "${number}.${number}.${number}";
// Something like ^1.3.3
type VersionRange = string;

type JsrScope = `@${string}`;
type JsrName = string;
type JsrPackage = `${JsrScope}/${JsrName}`;
type JsrSpecifierWithoutVersion = `jsr:${JsrPackage}`;
type JsrSpecifierWithSpecificVersion =
  `${JsrSpecifierWithoutVersion}@${SpecificVersion}`;
type JsrSpecifierWithVersionRange =
  `${JsrSpecifierWithoutVersion}@${VersionRange}`;
type JsrSpecifier = JsrSpecifierWithoutVersion | JsrSpecifierWithVersionRange;

type NpmScope = `@${string}`;
type NpmName = string;
type NpmPackage = `${NpmScope}/${NpmName}` | NpmName;
type NpmSpecifierWithoutVersion = `npm:${NpmPackage}`;
type NpmSpecifierWithSpecificVersion =
  `${NpmSpecifierWithoutVersion}@${SpecificVersion}`;
type NpmSpecifierWithVersionRange =
  `${NpmSpecifierWithoutVersion}@${VersionRange}`;
type NpmSpecifier = NpmSpecifierWithoutVersion | NpmSpecifierWithVersionRange;

type NpmSpecifierWithoutProtocol = NpmPackage | `${NpmPackage}@${VersionRange}`;
type RenamedNpmSpecifier = `${NpmPackage}@${NpmSpecifier}`;

type HttpsUrl = "https://${string}";

type Lockfile4 = {
  version: "4";
  // Map of incomplete specifiers to the exact version that will be used
  // If there is only one version of a package, we dont need an entry here, but implicitly use that version
  specifiers?: {
    [key: JsrSpecifier]: JsrSpecifierWithSpecificVersion;
    [key: NpmSpecifier]: NpmSpecifierWithSpecificVersion;
  };
  // Map of all locked jsr dependencies
  jsr?: {
    [key: JsrSpecifierWithSpecificVersion]: {
      // SHA256 hash of the version metadata file
      // Example: "09154a97e18c4d6a1692e3b3c8a3b1ec2934f00b7c1caf7491d762d963ada045"
      integrity: string;
      // Undefined if there are no dependencies. Empty Array is not valid
      // Example: ["jsr:@scope/name", "jsr:@scope/name@^0.2.2"]
      dependencies?: (JsrSpecifier | NpmSpecifier)[];
    };
  };
  // Map of all locked npm dependencies
  npm?: {
    [key: NpmSpecifierWithSpecificVersion]: {
      // Hash of the package. Usually a sha512
      // TODO: Figure out how this hash is generated
      // Example: "sha512-vq24Bq3ym5HEQm2NKCr3yXDwjc7vTsEThRDnkp2DK9p1uqLR+DHurm/NOTo0KG7HYHU7eppKZj3MyqYuMBf62g=="
      integrity: string;
      // Undefined if there are no dependencies. Empty Array is not valid
      // If we only have one version of a package in the dependency graph, the version should/will be omitted
      // Example: ["@scope/name", "@scope/name@^0.2.2", "name", "alias:@scope/name@0.2.2" ]
      dependencies?: (NpmSpecifierWithoutProtocol | RenamedNpmSpecifier)[];
    };
  };
  // The list of all redirects for https dependencies
  redirects?: Record<HttpsUrl, HttpsUrl>;
  // The list of all fetched files and their SHA-256 checksums
  // If a URL returned a redirect, the redirect target URL will be listed here
  // Example: "https://deno.land/std@0.120.0/async/deadline.ts": "1d6ac7aeaee22f75eb86e4e105d6161118aad7b41ae2dd14f4cfd3bf97472b93"
  remote?: Record<HttpsUrl, string>;
  // Some tracked information about the workspace
  workspace?: {
    // jsr dependencies
    dependencies?: Array<JsrSpecifier>;
    // npm dependencies
    packageJson?: {
      dependencies: Array<NpmSpecifier>;
    };
    // Members of the workspace
    members?: Record<
      string,
      {
        // jsr dependencies of the member
        dependencies?: Array<JsrSpecifier>;
        // npm dependencies of the member
        packageJson?: {
          dependencies: Array<NpmSpecifier>;
        };
      }
    >;
  };
};
```
