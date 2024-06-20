import {
  assertEquals,
  assertExists,
  assertFalse,
  assertObjectMatch,
} from "@std/assert";
import { beforeEach, describe, it } from "@std/testing/bdd";
import { instantiate, Lockfile, parseFromJson } from "./mod.ts";

describe("parseFromJson", () => {
  const json = {
    version: "3",
    packages: {
      "specifiers": {
        "jsr:@std/testing@^0.225.2": "jsr:@std/testing@0.225.2",
      },
      jsr: {
        "@std/testing@0.225.2": {
          "integrity":
            "ae5a55e412926acdba98ec3b72aba93d2ab40cad3d0cd2454b048c10ca3584d8",
        },
      },
    },
    remote: {},
  };

  it("should parse a LockFileJson and return a LockFile", async () => {
    const lockfile = await parseFromJson(
      "file:///deno.lock",
      json,
    );
    assertExists(lockfile);
  });

  it("should parse a stringified LockFileJson and return a LockFile", async () => {
    const lockfile = await parseFromJson(
      "file:///deno.lock",
      JSON.stringify(json),
    );
    assertExists(lockfile);
  });
});

describe("instantiate", () => {
  it("should return a synchronous interface to parseFromJson", async () => {
    const wasm = await instantiate();
    assertEquals(
      wasm.parseFromJson("file:///deno.lock", {
        version: "3",
        remote: {},
      }).toJson(),
      {
        version: "3",
        remote: {},
      },
    );
  });
});

describe("LockFile", () => {
  describe("filename", () => {
    it("should return the filename", async () => {
      const lockfile = await parseFromJson("file:///deno.lock", {
        version: "3",
        remote: {},
      });
      assertEquals(lockfile.filename, "file:///deno.lock");
    });
  });

  describe("copy", () => {
    it("should copy a lockfile", async () => {
      const json = {
        version: "3",
        packages: {
          "specifiers": {
            "jsr:@std/testing@^0.225.2": "jsr:@std/testing@0.225.2",
          },
          jsr: {
            "@std/testing@0.225.2": {
              "integrity":
                "ae5a55e412926acdba98ec3b72aba93d2ab40cad3d0cd2454b048c10ca3584d8",
            },
          },
        },
        remote: {},
      };
      const original = await parseFromJson("file:///deno.lock", json);
      const copy = original.copy();
      assertEquals(copy.toJson(), json);

      copy.insertRemote("https://deno.land/std@0.224.0/version.ts", "xxx");
      assertObjectMatch(copy.toJson(), {
        remote: {
          "https://deno.land/std@0.224.0/version.ts": "xxx",
        },
      });
      assertEquals(original.toJson(), json);
    });
  });

  describe("setWorkspaceConfig", () => {
    let lockfile: Lockfile;

    beforeEach(async () => {
      lockfile = await parseFromJson(
        "file:///deno.lock",
        {
          version: "3",
          packages: {
            specifiers: {
              "jsr:@std/assert@^0.226.0": "jsr:@std/assert@0.226.0",
              "jsr:@std/internal@^1.0.0": "jsr:@std/internal@1.0.0",
              "jsr:@std/testing@^0.225.2": "jsr:@std/testing@0.225.2",
            },
            jsr: {
              "@std/assert@0.226.0": {
                integrity:
                  "0dfb5f7c7723c18cec118e080fec76ce15b4c31154b15ad2bd74822603ef75b3",
                dependencies: [
                  "jsr:@std/internal@^1.0.0",
                ],
              },
              "@std/internal@1.0.0": {
                integrity:
                  "ac6a6dfebf838582c4b4f61a6907374e27e05bedb6ce276e0f1608fe84e7cd9a",
              },
              "@std/testing@0.225.2": {
                integrity:
                  "ae5a55e412926acdba98ec3b72aba93d2ab40cad3d0cd2454b048c10ca3584d8",
              },
            },
          },
          remote: {},
          workspace: {
            dependencies: [
              "jsr:@std/assert@^0.226.0",
              "jsr:@std/testing@^0.225.2",
            ],
            members: {
              "@deno/lockfile": {},
            },
          },
        },
      );
    });

    it("should remove all dependencies from a lockfile", () => {
      lockfile.setWorkspaceConfig({ dependencies: [] });
      const actual = lockfile.toJson();
      assertFalse("packages" in actual);
      assertFalse("workspace" in actual);
    });

    it("should retain a specific dependency from a lockfile", () => {
      lockfile.setWorkspaceConfig({
        dependencies: ["jsr:@std/assert@^0.226.0"],
      });
      const actual = lockfile.toJson();
      assertObjectMatch(actual, {
        version: "3",
        packages: {
          specifiers: {
            "jsr:@std/assert@^0.226.0": "jsr:@std/assert@0.226.0",
            "jsr:@std/internal@^1.0.0": "jsr:@std/internal@1.0.0",
          },
          jsr: {
            "@std/assert@0.226.0": {
              integrity:
                "0dfb5f7c7723c18cec118e080fec76ce15b4c31154b15ad2bd74822603ef75b3",
              dependencies: ["jsr:@std/internal@^1.0.0"],
            },
            "@std/internal@1.0.0": {
              integrity:
                "ac6a6dfebf838582c4b4f61a6907374e27e05bedb6ce276e0f1608fe84e7cd9a",
            },
          },
        },
        remote: {},
        workspace: { dependencies: ["jsr:@std/assert@^0.226.0"] },
      });
    });
  });

  describe("insertRemote", () => {
    it("should insert a remote dependency", async () => {
      const lockfile = await parseFromJson("file:///deno.lock", {
        version: "3",
        remote: {},
      });
      lockfile.insertRemote("https://deno.land/std@0.224.0/version.ts", "xxx");
      assertObjectMatch(lockfile.toJson(), {
        version: "3",
        remote: {
          "https://deno.land/std@0.224.0/version.ts": "xxx",
        },
      });
    });
  });

  describe("insertNpmPackage", () => {
    it("should insert an npm package", async () => {
      const lockfile = await parseFromJson("file:///deno.lock", {
        version: "3",
        remote: {},
      });
      const npmPackageInfo = {
        "integrity":
          "sha512-BLI3Tl1TW3Pvl70l3yq3Y64i+awpwXqsGBYWkkqMtnbXgrMD+yj7rhW0kuEDxzJaYXGjEW5ogapKNMEKNMjibA==",
        "dependencies": {
          "isexe": "isexe@2.0.0",
        },
      };
      lockfile.insertNpmPackage("which@2.0.2", npmPackageInfo);
      assertObjectMatch(lockfile.toJson(), {
        packages: {
          npm: {
            "which@2.0.2": npmPackageInfo,
          },
        },
      });
    });
  });

  describe("insertPackageSpecifier", () => {
    it("should insert a jsr package specifier", async () => {
      const lockfile = await parseFromJson("file:///deno.lock", {
        version: "3",
        remote: {},
      });
      lockfile.insertPackageSpecifier(
        "jsr:@std/testing@^0.225.0",
        "jsr:@std/testing@0.225.2",
      );
      assertObjectMatch(lockfile.toJson(), {
        packages: {
          specifiers: {
            "jsr:@std/testing@^0.225.0": "jsr:@std/testing@0.225.2",
          },
        },
      });
    });

    it("should insert a npm package specifier", async () => {
      const lockfile = await parseFromJson("file:///deno.lock", {
        version: "3",
        remote: {},
      });
      lockfile.insertPackageSpecifier("npm:which@^2.0.0", "npm:which@2.0.2");
      assertObjectMatch(lockfile.toJson(), {
        packages: {
          specifiers: {
            "npm:which@^2.0.0": "npm:which@2.0.2",
          },
        },
      });
    });
  });

  describe("insertPackage", () => {
    it("should insert a jsr package", async () => {
      const lockfile = await parseFromJson("file:///deno.lock", {
        version: "3",
        remote: {},
      });
      const specifier = "jsr:@std/assert@0.226.0";
      const integrity =
        "0dfb5f7c7723c18cec118e080fec76ce15b4c31154b15ad2bd74822603ef75b3";
      lockfile.insertPackage(specifier, integrity);
      assertObjectMatch(lockfile.toJson(), {
        packages: {
          jsr: {
            [specifier]: { integrity },
          },
        },
      });
    });
  });

  describe("addPackageDeps", () => {
    it("should add dependencies of a jsr package", async () => {
      const lockfile = await parseFromJson("file:///deno.lock", {
        version: "3",
        remote: {},
      });
      const specifier = "jsr:@std/assert@0.226.0";
      const integrity =
        "0dfb5f7c7723c18cec118e080fec76ce15b4c31154b15ad2bd74822603ef75b3";
      const dependencies = ["@std/internal@^1.0.0"];
      lockfile.insertPackage(specifier, integrity);
      lockfile.addPackageDeps(specifier, dependencies);
      assertObjectMatch(lockfile.toJson(), {
        packages: {
          jsr: {
            [specifier]: { integrity, dependencies },
          },
        },
      });
    });
  });

  describe("insertRedirect", () => {
    it("should insert a redirect", async () => {
      const lockfile = await parseFromJson("file:///deno.lock", {
        version: "3",
        remote: {},
      });
      const from = "https://deno.land/x/std/mod.ts";
      const to = "https://deno.land/std@0.224.0/mod.ts";
      lockfile.insertRedirect(from, to);
      assertObjectMatch(lockfile.toJson(), {
        redirects: {
          [from]: to,
        },
      });
    });
  });
});
