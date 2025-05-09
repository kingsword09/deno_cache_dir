// Copyright 2018-2025 Kingsword. MIT license.

import { isAbsolute } from "@std/path";
import { assert } from "./util.ts";
import { ReadOnlyNpmCache } from "./lib/deno_cache_dir.js";

export interface NpmCacheCreateOptions {
  root?: string;
  known_registries?: string[];
}

export interface NpmCacheFolderId {
  // package name
  name: string;
  // package version
  version: string;
  // Package copy index.
  copy_index: number;
  // Package registry URL.
  registry_url: string;
}

export class NpmCache implements Disposable {
  #cache: ReadOnlyNpmCache;

  private constructor(cache: ReadOnlyNpmCache) {
    this.#cache = cache;
  }

  static async create(options: NpmCacheCreateOptions): Promise<NpmCache> {
    let cache: ReadOnlyNpmCache;
    if (options.root != null) {
      cache = ReadOnlyNpmCache.new(options.root, options.known_registries);
    } else {
      cache = ReadOnlyNpmCache.from_deno_dir(options.known_registries);
    }
    return new NpmCache(cache);
  }

  [Symbol.dispose]() {
    this.free();
  }

  free() {
    this.#cache?.free();
  }

  get_cache_location(): string {
    return this.#cache.get_cache_location();
  }

  resolve_package_folder_id_from_specifier(
    specifier: string
  ): NpmCacheFolderId | undefined {
    return this.#cache.resolve_package_folder_id_from_specifier(specifier);
  }

  package_folder_for_id(
    package_name: string,
    package_version: string,
    package_copy_index: number,
    registry_url?: string | null
  ): string {
    return this.#cache.package_folder_for_id(
      package_name,
      package_version,
      package_copy_index,
      registry_url
    );
  }
}
