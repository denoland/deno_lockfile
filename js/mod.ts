import {
  instantiate,
  type JsLockFile,
} from "./deno_lockfile_wasm.generated.js";

export interface WorkspaceMemberConfig {
  dependencies?: string[];
  packageJson?: {
    dependencies: string[];
  };
}

export interface WorkspaceConfig extends WorkspaceMemberConfig {
  members?: Record<string, WorkspaceMemberConfig>;
}

export interface LockFileJson {
  version: string;
  packages?: {
    specifiers: Record<string, string>;
    jsr?: Record<string, JsrPackageInfo>;
    npm?: Record<string, NpmPackageInfo>;
  };
  redirects?: Record<string, string>;
  remote?: Record<string, string>;
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

export interface LockFile extends Omit<JsLockFile, "free"> {
  toJson(): LockFileJson;
  insertNpmPackage(specifier: string, packageInfo: NpmPackageInfo): void;
  setWorkspaceConfig(config: WorkspaceConfig): void;
}

export async function parseFromJson(
  baseUrl: string | URL,
  json: string | LockFileJson,
): Promise<LockFile> {
  const wasm = await instantiate();
  if (baseUrl instanceof URL) {
    baseUrl = baseUrl.toString();
  }
  if (typeof json === "object") {
    json = JSON.stringify(json);
  }
  return wasm.parseFromJson(baseUrl, json);
}
