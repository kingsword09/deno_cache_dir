// Copyright 2018-2025 the Deno authors. MIT license.

use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;

use deno_path_util::normalize_path;
use deno_path_util::url_from_directory_path;
use sys_traits::FsCanonicalize;
use sys_traits::FsCreateDirAll;
use url::Url;

pub struct NpmCacheFolderId {
  /// Package name.
  pub name: String,
  /// Package version.
  pub version: String,
  /// Package copy index.
  pub copy_index: u8,
  /// Package registry url.
  pub registry_url: String,
}

/// The global cache directory of npm packages.
#[derive(Clone, Debug)]
pub struct NpmCacheDir {
  root_dir: PathBuf,
  // cached url representation of the root directory
  root_dir_url: Url,
  // A list of all registry that were discovered via `.npmrc` files
  // turned into a safe directory names.
  known_registries_dirnames: Vec<String>,
}

impl NpmCacheDir {
  pub fn new<Sys: FsCanonicalize + FsCreateDirAll>(
    sys: &Sys,
    root_dir: PathBuf,
    known_registries_urls: Vec<Url>,
  ) -> Self {
    fn try_get_canonicalized_root_dir<Sys: FsCanonicalize + FsCreateDirAll>(
      sys: &Sys,
      root_dir: &Path,
    ) -> Result<PathBuf, std::io::Error> {
      match sys.fs_canonicalize(root_dir) {
        Ok(path) => Ok(path),
        Err(err) if err.kind() == ErrorKind::NotFound => {
          sys.fs_create_dir_all(root_dir)?;
          sys.fs_canonicalize(root_dir)
        }
        Err(err) => Err(err),
      }
    }

    // this may fail on readonly file systems, so just ignore if so
    let root_dir = normalize_path(root_dir);
    let root_dir =
      try_get_canonicalized_root_dir(sys, &root_dir).unwrap_or(root_dir);
    let root_dir_url = url_from_directory_path(&root_dir).unwrap();

    let known_registries_dirnames: Vec<_> = known_registries_urls
      .into_iter()
      .map(|url| {
        root_url_to_safe_local_dirname(&url)
          .to_string_lossy()
          .replace('\\', "/")
      })
      .collect();

    Self {
      root_dir,
      root_dir_url,
      known_registries_dirnames,
    }
  }

  pub fn root_dir(&self) -> &Path {
    &self.root_dir
  }

  pub fn root_dir_url(&self) -> &Url {
    &self.root_dir_url
  }

  pub fn package_folder_for_id(
    &self,
    package_name: &str,
    package_version: &str,
    package_copy_index: u8,
    registry_url: &Url,
  ) -> PathBuf {
    if package_copy_index == 0 {
      self
        .package_name_folder(package_name, registry_url)
        .join(package_version)
    } else {
      self
        .package_name_folder(package_name, registry_url)
        .join(format!("{}_{}", package_version, package_copy_index))
    }
  }

  pub fn package_name_folder(&self, name: &str, registry_url: &Url) -> PathBuf {
    let mut dir = self.registry_folder(registry_url);
    if name.to_lowercase() != name {
      let encoded_name = mixed_case_package_name_encode(name);
      // Using the encoded directory may have a collision with an actual package name
      // so prefix it with an underscore since npm packages can't start with that
      dir.join(format!("_{encoded_name}"))
    } else {
      // ensure backslashes are used on windows
      for part in name.split('/') {
        dir = dir.join(part);
      }
      dir
    }
  }

  fn registry_folder(&self, registry_url: &Url) -> PathBuf {
    self
      .root_dir
      .join(root_url_to_safe_local_dirname(registry_url))
  }

  pub fn resolve_package_folder_id_from_specifier(
    &self,
    specifier: &Url,
  ) -> Option<NpmCacheFolderId> {
    // Try to find matching registry and parse package info
    for registry_dirname in &self.known_registries_dirnames {
      let registry_root_dir = self
        .root_dir_url
        .join(&format!("{}/", registry_dirname))
        .unwrap();

      // Get relative path from registry root to package
      let relative_url = match registry_root_dir.make_relative(specifier) {
        Some(url) if !url.starts_with("../") => url,
        _ => continue,
      };

      // Handle base32 encoded package names
      let relative_url = if let Some(end_url) = relative_url.strip_prefix('_') {
        let mut parts: Vec<String> = end_url.split('/').map(String::from).collect();
        parts[0] = mixed_case_package_name_decode(&parts[0])?;
        parts.join("/")
      } else {
        relative_url
      };

      // Parse package name and version
      let is_scoped = relative_url.starts_with('@');
      let parts: Vec<_> = relative_url
        .split('/')
        .take(if is_scoped { 3 } else { 2 })
        .collect();
      
      if parts.len() < 2 {
        continue;
      }

      let version_part = parts.last().unwrap();
      let name = parts[..parts.len()-1].join("/");

      // Parse version and copy index
      let (version, copy_index) = match version_part.split_once('_') {
        Some((ver, idx)) => (ver, idx.parse::<u8>().ok()?),
        None => (*version_part, 0),
      };

      return Some(NpmCacheFolderId {
        name,
        version: version.to_string(),
        copy_index,
        registry_url: relative_url,
      });
    }

    None
  }

  pub fn get_cache_location(&self) -> PathBuf {
    self.root_dir.clone()
  }
}

pub fn mixed_case_package_name_encode(name: &str) -> String {
  // use base32 encoding because it's reversible and the character set
  // only includes the characters within 0-9 and A-Z so it can be lower cased
  base32::encode(
    base32::Alphabet::Rfc4648Lower { padding: false },
    name.as_bytes(),
  )
  .to_lowercase()
}

pub fn mixed_case_package_name_decode(name: &str) -> Option<String> {
  base32::decode(base32::Alphabet::Rfc4648Lower { padding: false }, name)
    .and_then(|b| String::from_utf8(b).ok())
}

/// Gets a safe local directory name for the provided url.
///
/// For example:
/// https://deno.land:8080/path -> deno.land_8080/path
fn root_url_to_safe_local_dirname(root: &Url) -> PathBuf {
  fn sanitize_segment(text: &str) -> String {
    text
      .chars()
      .map(|c| if is_banned_segment_char(c) { '_' } else { c })
      .collect()
  }

  fn is_banned_segment_char(c: char) -> bool {
    matches!(c, '/' | '\\') || is_banned_path_char(c)
  }

  let mut result = String::new();
  if let Some(domain) = root.domain() {
    result.push_str(&sanitize_segment(domain));
  }
  if let Some(port) = root.port() {
    if !result.is_empty() {
      result.push('_');
    }
    result.push_str(&port.to_string());
  }
  let mut result = PathBuf::from(result);
  if let Some(segments) = root.path_segments() {
    for segment in segments.filter(|s| !s.is_empty()) {
      result = result.join(sanitize_segment(segment));
    }
  }

  result
}

/// Gets if the provided character is not supported on all
/// kinds of file systems.
fn is_banned_path_char(c: char) -> bool {
  matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*')
}

#[cfg(test)]
mod test {
  use std::path::PathBuf;

  use sys_traits::FsCreateDirAll;
  use url::Url;

  use super::NpmCacheDir;

  #[test]
  fn should_get_package_folder() {
    let sys = sys_traits::impls::InMemorySys::default();
    let root_dir = if cfg!(windows) {
      PathBuf::from("C:\\cache")
    } else {
      PathBuf::from("/cache")
    };
    sys.fs_create_dir_all(&root_dir).unwrap();
    let registry_url = Url::parse("https://registry.npmjs.org/").unwrap();
    let cache =
      NpmCacheDir::new(&sys, root_dir.clone(), vec![registry_url.clone()]);

    assert_eq!(
      cache.package_folder_for_id("json", "1.2.5", 0, &registry_url,),
      root_dir
        .join("registry.npmjs.org")
        .join("json")
        .join("1.2.5"),
    );

    assert_eq!(
      cache.package_folder_for_id("json", "1.2.5", 1, &registry_url,),
      root_dir
        .join("registry.npmjs.org")
        .join("json")
        .join("1.2.5_1"),
    );

    assert_eq!(
      cache.package_folder_for_id("JSON", "2.1.5", 0, &registry_url,),
      root_dir
        .join("registry.npmjs.org")
        .join("_jjju6tq")
        .join("2.1.5"),
    );

    assert_eq!(
      cache.package_folder_for_id("@types/JSON", "2.1.5", 0, &registry_url,),
      root_dir
        .join("registry.npmjs.org")
        .join("_ib2hs4dfomxuuu2pjy")
        .join("2.1.5"),
    );
  }
}
