import * as wasm from "./deno_lockfile_wasm.generated.js";
import { type JsLockfile } from "./deno_lockfile_wasm.generated.d.ts";

export interface WorkspaceMemberConfig {
  dependencies?: string[];
  packageJson?: {
    dependencies: string[];
  };
}

export interface WorkspaceConfig extends WorkspaceMemberConfig {
  members?: Record<string, WorkspaceMemberConfig>;
}

export interface LockfileJson {
  version: string;
  packages?: {
    specifiers: Record<string, string>;
    jsr?: Record<string, JsrPackageInfo>;
    npm?: Record<string, NpmPackageInfo>;
  };
  redirects?: Record<string, string>;
  remote: Record<string, string>;
  workspace?: WorkspaceConfig;
}

export interface JsrPackageInfo {
  integrity: string;
  dependencies?: string[];
}

export interface NpmPackageInfo {
  integrity: string;
  dependencies: Record<string, string>;
}

export interface Lockfile
  extends Omit<JsLockfile, "copy" | "filename" | "free"> {
  copy(): Lockfile;
  insertNpmPackage(specifier: string, packageInfo: NpmPackageInfo): void;
  setWorkspaceConfig(config: WorkspaceConfig): void;
  toJson(): LockfileJson;
  get filename(): string;
}

export async function parseFromJson(
  baseUrl: string | URL,
  json: string | LockfileJson,
): Promise<Lockfile> {
  return parseFromJsonWith(baseUrl, json, await wasm.instantiate());
}

export interface InstanciateResult {
  parseFromJson(baseUrl: string | URL, json: string | LockfileJson): Lockfile;
}

export async function instantiate(
  opts?: wasm.InstantiateOptions,
): Promise<InstanciateResult> {
  const mod = await wasm.instantiate(opts);
  return {
    parseFromJson(baseUrl, json) {
      return parseFromJsonWith(baseUrl, json, mod);
    },
  };
}

function parseFromJsonWith(
  baseUrl: string | URL,
  json: string | LockfileJson,
  mod: Awaited<ReturnType<typeof wasm.instantiate>>,
): Lockfile {
  if (baseUrl instanceof URL) {
    baseUrl = baseUrl.toString();
  }
  if (typeof json === "object") {
    json = JSON.stringify(json);
  }
  const inner = mod.parseFromJson(baseUrl, json);
  return new Proxy(inner, {
    get(target, prop, receiver) {
      if (prop === "filename") {
        return inner.filename();
      }
      return Reflect.get(target, prop, receiver);
    },
  }) as unknown as Lockfile;
}
