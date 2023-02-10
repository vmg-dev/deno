// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.

use deno_core::error::generic_error;
use deno_core::error::AnyError;
use deno_core::include_js_files;
use deno_core::normalize_path;
use deno_core::op;
use deno_core::url::Url;
use deno_core::Extension;
use deno_core::JsRuntimeInspector;
use deno_core::OpState;
use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;

pub mod errors;
mod package_json;
mod path;
mod resolution;

pub use package_json::PackageJson;
pub use path::PathClean;
pub use resolution::get_closest_package_json;
pub use resolution::get_package_scope_config;
pub use resolution::legacy_main_resolve;
pub use resolution::package_exports_resolve;
pub use resolution::package_imports_resolve;
pub use resolution::package_resolve;
pub use resolution::path_to_declaration_path;
pub use resolution::NodeModuleKind;
pub use resolution::NodeResolutionMode;
pub use resolution::DEFAULT_CONDITIONS;
use std::cell::RefCell;

pub trait NodePermissions {
  fn check_read(&mut self, path: &Path) -> Result<(), AnyError>;
}

pub trait RequireNpmResolver {
  fn resolve_package_folder_from_package(
    &self,
    specifier: &str,
    referrer: &Path,
    mode: NodeResolutionMode,
  ) -> Result<PathBuf, AnyError>;

  fn resolve_package_folder_from_path(
    &self,
    path: &Path,
  ) -> Result<PathBuf, AnyError>;

  fn in_npm_package(&self, path: &Path) -> bool;

  fn ensure_read_permission(
    &self,
    permissions: &mut dyn NodePermissions,
    path: &Path,
  ) -> Result<(), AnyError>;
}

pub const MODULE_ES_SHIM: &str = include_str!("./module_es_shim.js");

pub static NODE_GLOBAL_THIS_NAME: Lazy<String> = Lazy::new(|| {
  let now = std::time::SystemTime::now();
  let seconds = now
    .duration_since(std::time::SystemTime::UNIX_EPOCH)
    .unwrap()
    .as_secs();
  // use a changing variable name to make it hard to depend on this
  format!("__DENO_NODE_GLOBAL_THIS_{seconds}__")
});

pub static NODE_ENV_VAR_ALLOWLIST: Lazy<HashSet<String>> = Lazy::new(|| {
  // The full list of environment variables supported by Node.js is available
  // at https://nodejs.org/api/cli.html#environment-variables
  let mut set = HashSet::new();
  set.insert("NODE_DEBUG".to_string());
  set.insert("NODE_OPTIONS".to_string());
  set
});

pub fn init<P: NodePermissions + 'static>(
  maybe_npm_resolver: Option<Rc<dyn RequireNpmResolver>>,
) -> Extension {
  Extension::builder(env!("CARGO_PKG_NAME"))
    .esm(include_js_files!(
      "01_node.js",
      "02_require.js",
      "module_es_shim.js",
    ))
    .ops(vec![
      op_require_init_paths::decl(),
      op_require_node_module_paths::decl::<P>(),
      op_require_proxy_path::decl(),
      op_require_is_deno_dir_package::decl(),
      op_require_resolve_deno_dir::decl(),
      op_require_is_request_relative::decl(),
      op_require_resolve_lookup_paths::decl(),
      op_require_try_self_parent_path::decl::<P>(),
      op_require_try_self::decl::<P>(),
      op_require_real_path::decl::<P>(),
      op_require_path_is_absolute::decl(),
      op_require_path_dirname::decl(),
      op_require_stat::decl::<P>(),
      op_require_path_resolve::decl(),
      op_require_path_basename::decl(),
      op_require_read_file::decl::<P>(),
      op_require_as_file_path::decl(),
      op_require_resolve_exports::decl::<P>(),
      op_require_read_closest_package_json::decl::<P>(),
      op_require_read_package_scope::decl::<P>(),
      op_require_package_imports_resolve::decl::<P>(),
      op_require_break_on_next_statement::decl(),
    ])
    .state(move |state| {
      if let Some(npm_resolver) = maybe_npm_resolver.clone() {
        state.put(npm_resolver);
      }
      Ok(())
    })
    .build()
}

fn ensure_read_permission<P>(
  state: &mut OpState,
  file_path: &Path,
) -> Result<(), AnyError>
where
  P: NodePermissions + 'static,
{
  let resolver = {
    let resolver = state.borrow::<Rc<dyn RequireNpmResolver>>();
    resolver.clone()
  };
  let permissions = state.borrow_mut::<P>();
  resolver.ensure_read_permission(permissions, file_path)
}

#[op]
pub fn op_require_init_paths() -> Vec<String> {
  // todo(dsherret): this code is node compat mode specific and
  // we probably don't want it for small mammal, so ignore it for now

  // let (home_dir, node_path) = if cfg!(windows) {
  //   (
  //     std::env::var("USERPROFILE").unwrap_or_else(|_| "".into()),
  //     std::env::var("NODE_PATH").unwrap_or_else(|_| "".into()),
  //   )
  // } else {
  //   (
  //     std::env::var("HOME").unwrap_or_else(|_| "".into()),
  //     std::env::var("NODE_PATH").unwrap_or_else(|_| "".into()),
  //   )
  // };

  // let mut prefix_dir = std::env::current_exe().unwrap();
  // if cfg!(windows) {
  //   prefix_dir = prefix_dir.join("..").join("..")
  // } else {
  //   prefix_dir = prefix_dir.join("..")
  // }

  // let mut paths = vec![prefix_dir.join("lib").join("node")];

  // if !home_dir.is_empty() {
  //   paths.insert(0, PathBuf::from(&home_dir).join(".node_libraries"));
  //   paths.insert(0, PathBuf::from(&home_dir).join(".nod_modules"));
  // }

  // let mut paths = paths
  //   .into_iter()
  //   .map(|p| p.to_string_lossy().to_string())
  //   .collect();

  // if !node_path.is_empty() {
  //   let delimiter = if cfg!(windows) { ";" } else { ":" };
  //   let mut node_paths: Vec<String> = node_path
  //     .split(delimiter)
  //     .filter(|e| !e.is_empty())
  //     .map(|s| s.to_string())
  //     .collect();
  //   node_paths.append(&mut paths);
  //   paths = node_paths;
  // }

  vec![]
}

#[op]
pub fn op_require_node_module_paths<P>(
  state: &mut OpState,
  from: String,
) -> Result<Vec<String>, AnyError>
where
  P: NodePermissions + 'static,
{
  // Guarantee that "from" is absolute.
  let from = deno_core::resolve_path(&from)
    .unwrap()
    .to_file_path()
    .unwrap();

  ensure_read_permission::<P>(state, &from)?;

  if cfg!(windows) {
    // return root node_modules when path is 'D:\\'.
    let from_str = from.to_str().unwrap();
    if from_str.len() >= 3 {
      let bytes = from_str.as_bytes();
      if bytes[from_str.len() - 1] == b'\\' && bytes[from_str.len() - 2] == b':'
      {
        let p = from_str.to_owned() + "node_modules";
        return Ok(vec![p]);
      }
    }
  } else {
    // Return early not only to avoid unnecessary work, but to *avoid* returning
    // an array of two items for a root: [ '//node_modules', '/node_modules' ]
    if from.to_string_lossy() == "/" {
      return Ok(vec!["/node_modules".to_string()]);
    }
  }

  let mut paths = vec![];
  let mut current_path = from.as_path();
  let mut maybe_parent = Some(current_path);
  while let Some(parent) = maybe_parent {
    if !parent.ends_with("/node_modules") {
      paths.push(parent.join("node_modules").to_string_lossy().to_string());
      current_path = parent;
      maybe_parent = current_path.parent();
    }
  }

  if !cfg!(windows) {
    // Append /node_modules to handle root paths.
    paths.push("/node_modules".to_string());
  }

  Ok(paths)
}

#[op]
fn op_require_proxy_path(filename: String) -> String {
  // Allow a directory to be passed as the filename
  let trailing_slash = if cfg!(windows) {
    // Node also counts a trailing forward slash as a
    // directory for node on Windows, but not backslashes
    // on non-Windows platforms
    filename.ends_with('\\') || filename.ends_with('/')
  } else {
    filename.ends_with('/')
  };

  if trailing_slash {
    let p = PathBuf::from(filename);
    p.join("noop.js").to_string_lossy().to_string()
  } else {
    filename
  }
}

#[op]
fn op_require_is_request_relative(request: String) -> bool {
  if request.starts_with("./") || request.starts_with("../") || request == ".."
  {
    return true;
  }

  if cfg!(windows) {
    if request.starts_with(".\\") {
      return true;
    }

    if request.starts_with("..\\") {
      return true;
    }
  }

  false
}

#[op]
fn op_require_resolve_deno_dir(
  state: &mut OpState,
  request: String,
  parent_filename: String,
) -> Option<String> {
  let resolver = state.borrow::<Rc<dyn RequireNpmResolver>>();
  resolver
    .resolve_package_folder_from_package(
      &request,
      &PathBuf::from(parent_filename),
      NodeResolutionMode::Execution,
    )
    .ok()
    .map(|p| p.to_string_lossy().to_string())
}

#[op]
fn op_require_is_deno_dir_package(state: &mut OpState, path: String) -> bool {
  let resolver = state.borrow::<Rc<dyn RequireNpmResolver>>();
  resolver.in_npm_package(&PathBuf::from(path))
}

#[op]
fn op_require_resolve_lookup_paths(
  request: String,
  maybe_parent_paths: Option<Vec<String>>,
  parent_filename: String,
) -> Option<Vec<String>> {
  if !request.starts_with('.')
    || (request.len() > 1
      && !request.starts_with("..")
      && !request.starts_with("./")
      && (!cfg!(windows) || !request.starts_with(".\\")))
  {
    let module_paths = vec![];
    let mut paths = module_paths;
    if let Some(mut parent_paths) = maybe_parent_paths {
      if !parent_paths.is_empty() {
        paths.append(&mut parent_paths);
      }
    }

    if !paths.is_empty() {
      return Some(paths);
    } else {
      return None;
    }
  }

  // In REPL, parent.filename is null.
  // if (!parent || !parent.id || !parent.filename) {
  //   // Make require('./path/to/foo') work - normally the path is taken
  //   // from realpath(__filename) but in REPL there is no filename
  //   const mainPaths = ['.'];

  //   debug('looking for %j in %j', request, mainPaths);
  //   return mainPaths;
  // }

  let p = PathBuf::from(parent_filename);
  Some(vec![p.parent().unwrap().to_string_lossy().to_string()])
}

#[op]
fn op_require_path_is_absolute(p: String) -> bool {
  PathBuf::from(p).is_absolute()
}

#[op]
fn op_require_stat<P>(
  state: &mut OpState,
  path: String,
) -> Result<i32, AnyError>
where
  P: NodePermissions + 'static,
{
  let path = PathBuf::from(path);
  ensure_read_permission::<P>(state, &path)?;
  if let Ok(metadata) = std::fs::metadata(&path) {
    if metadata.is_file() {
      return Ok(0);
    } else {
      return Ok(1);
    }
  }

  Ok(-1)
}

#[op]
fn op_require_real_path<P>(
  state: &mut OpState,
  request: String,
) -> Result<String, AnyError>
where
  P: NodePermissions + 'static,
{
  let path = PathBuf::from(request);
  ensure_read_permission::<P>(state, &path)?;
  let mut canonicalized_path = path.canonicalize()?;
  if cfg!(windows) {
    canonicalized_path = PathBuf::from(
      canonicalized_path
        .display()
        .to_string()
        .trim_start_matches("\\\\?\\"),
    );
  }
  Ok(canonicalized_path.to_string_lossy().to_string())
}

fn path_resolve(parts: Vec<String>) -> String {
  assert!(!parts.is_empty());
  let mut p = PathBuf::from(&parts[0]);
  if parts.len() > 1 {
    for part in &parts[1..] {
      p = p.join(part);
    }
  }
  normalize_path(p).to_string_lossy().to_string()
}

#[op]
fn op_require_path_resolve(parts: Vec<String>) -> String {
  path_resolve(parts)
}

#[op]
fn op_require_path_dirname(request: String) -> Result<String, AnyError> {
  let p = PathBuf::from(request);
  if let Some(parent) = p.parent() {
    Ok(parent.to_string_lossy().to_string())
  } else {
    Err(generic_error("Path doesn't have a parent"))
  }
}

#[op]
fn op_require_path_basename(request: String) -> Result<String, AnyError> {
  let p = PathBuf::from(request);
  if let Some(path) = p.file_name() {
    Ok(path.to_string_lossy().to_string())
  } else {
    Err(generic_error("Path doesn't have a file name"))
  }
}

#[op]
fn op_require_try_self_parent_path<P>(
  state: &mut OpState,
  has_parent: bool,
  maybe_parent_filename: Option<String>,
  maybe_parent_id: Option<String>,
) -> Result<Option<String>, AnyError>
where
  P: NodePermissions + 'static,
{
  if !has_parent {
    return Ok(None);
  }

  if let Some(parent_filename) = maybe_parent_filename {
    return Ok(Some(parent_filename));
  }

  if let Some(parent_id) = maybe_parent_id {
    if parent_id == "<repl>" || parent_id == "internal/preload" {
      if let Ok(cwd) = std::env::current_dir() {
        ensure_read_permission::<P>(state, &cwd)?;
        return Ok(Some(cwd.to_string_lossy().to_string()));
      }
    }
  }
  Ok(None)
}

#[op]
fn op_require_try_self<P>(
  state: &mut OpState,
  parent_path: Option<String>,
  request: String,
) -> Result<Option<String>, AnyError>
where
  P: NodePermissions + 'static,
{
  if parent_path.is_none() {
    return Ok(None);
  }

  let resolver = state.borrow::<Rc<dyn RequireNpmResolver>>().clone();
  let permissions = state.borrow_mut::<P>();
  let pkg = resolution::get_package_scope_config(
    &Url::from_file_path(parent_path.unwrap()).unwrap(),
    &*resolver,
    permissions,
  )
  .ok();
  if pkg.is_none() {
    return Ok(None);
  }

  let pkg = pkg.unwrap();
  if pkg.exports.is_none() {
    return Ok(None);
  }
  if pkg.name.is_none() {
    return Ok(None);
  }

  let pkg_name = pkg.name.as_ref().unwrap().to_string();
  let mut expansion = ".".to_string();

  if request == pkg_name {
    // pass
  } else if request.starts_with(&format!("{pkg_name}/")) {
    expansion += &request[pkg_name.len()..];
  } else {
    return Ok(None);
  }

  let referrer = deno_core::url::Url::from_file_path(&pkg.path).unwrap();
  if let Some(exports) = &pkg.exports {
    resolution::package_exports_resolve(
      &pkg.path,
      expansion,
      exports,
      &referrer,
      NodeModuleKind::Cjs,
      resolution::REQUIRE_CONDITIONS,
      NodeResolutionMode::Execution,
      &*resolver,
      permissions,
    )
    .map(|r| Some(r.to_string_lossy().to_string()))
  } else {
    Ok(None)
  }
}

#[op]
fn op_require_read_file<P>(
  state: &mut OpState,
  file_path: String,
) -> Result<String, AnyError>
where
  P: NodePermissions + 'static,
{
  let file_path = PathBuf::from(file_path);
  ensure_read_permission::<P>(state, &file_path)?;
  Ok(std::fs::read_to_string(file_path)?)
}

#[op]
pub fn op_require_as_file_path(file_or_url: String) -> String {
  if let Ok(url) = Url::parse(&file_or_url) {
    if let Ok(p) = url.to_file_path() {
      return p.to_string_lossy().to_string();
    }
  }

  file_or_url
}

#[op]
fn op_require_resolve_exports<P>(
  state: &mut OpState,
  uses_local_node_modules_dir: bool,
  modules_path: String,
  _request: String,
  name: String,
  expansion: String,
  parent_path: String,
) -> Result<Option<String>, AnyError>
where
  P: NodePermissions + 'static,
{
  let resolver = state.borrow::<Rc<dyn RequireNpmResolver>>().clone();
  let permissions = state.borrow_mut::<P>();

  let pkg_path = if resolver.in_npm_package(&PathBuf::from(&modules_path))
    && !uses_local_node_modules_dir
  {
    modules_path
  } else {
    path_resolve(vec![modules_path, name])
  };
  let pkg = PackageJson::load(
    &*resolver,
    permissions,
    PathBuf::from(&pkg_path).join("package.json"),
  )?;

  if let Some(exports) = &pkg.exports {
    let referrer = Url::from_file_path(parent_path).unwrap();
    resolution::package_exports_resolve(
      &pkg.path,
      format!(".{expansion}"),
      exports,
      &referrer,
      NodeModuleKind::Cjs,
      resolution::REQUIRE_CONDITIONS,
      NodeResolutionMode::Execution,
      &*resolver,
      permissions,
    )
    .map(|r| Some(r.to_string_lossy().to_string()))
  } else {
    Ok(None)
  }
}

#[op]
fn op_require_read_closest_package_json<P>(
  state: &mut OpState,
  filename: String,
) -> Result<PackageJson, AnyError>
where
  P: NodePermissions + 'static,
{
  ensure_read_permission::<P>(
    state,
    PathBuf::from(&filename).parent().unwrap(),
  )?;
  let resolver = state.borrow::<Rc<dyn RequireNpmResolver>>().clone();
  let permissions = state.borrow_mut::<P>();
  resolution::get_closest_package_json(
    &Url::from_file_path(filename).unwrap(),
    &*resolver,
    permissions,
  )
}

#[op]
fn op_require_read_package_scope<P>(
  state: &mut OpState,
  package_json_path: String,
) -> Option<PackageJson>
where
  P: NodePermissions + 'static,
{
  let resolver = state.borrow::<Rc<dyn RequireNpmResolver>>().clone();
  let permissions = state.borrow_mut::<P>();
  let package_json_path = PathBuf::from(package_json_path);
  PackageJson::load(&*resolver, permissions, package_json_path).ok()
}

#[op]
fn op_require_package_imports_resolve<P>(
  state: &mut OpState,
  parent_filename: String,
  request: String,
) -> Result<Option<String>, AnyError>
where
  P: NodePermissions + 'static,
{
  let parent_path = PathBuf::from(&parent_filename);
  ensure_read_permission::<P>(state, &parent_path)?;
  let resolver = state.borrow::<Rc<dyn RequireNpmResolver>>().clone();
  let permissions = state.borrow_mut::<P>();
  let pkg = PackageJson::load(
    &*resolver,
    permissions,
    parent_path.join("package.json"),
  )?;

  if pkg.imports.is_some() {
    let referrer =
      deno_core::url::Url::from_file_path(&parent_filename).unwrap();
    let r = resolution::package_imports_resolve(
      &request,
      &referrer,
      NodeModuleKind::Cjs,
      resolution::REQUIRE_CONDITIONS,
      NodeResolutionMode::Execution,
      &*resolver,
      permissions,
    )
    .map(|r| Some(Url::from_file_path(r).unwrap().to_string()));
    state.put(resolver);
    r
  } else {
    Ok(None)
  }
}

#[op]
fn op_require_break_on_next_statement(state: &mut OpState) {
  let inspector = state.borrow::<Rc<RefCell<JsRuntimeInspector>>>();
  inspector
    .borrow_mut()
    .wait_for_session_and_break_on_next_statement()
}

pub enum NodeModulePolyfillSpecifier {
  /// An internal module specifier, like "internal:deno_node/assert.ts". The
  /// module must be either embedded in the binary or snapshotted.
  Embedded(&'static str),

  /// Specifier relative to the root of `deno_std` repo, like "node/assert.ts"
  StdNode(&'static str),
}

pub struct NodeModulePolyfill {
  /// Name of the module like "assert" or "timers/promises"
  pub name: &'static str,
  pub specifier: NodeModulePolyfillSpecifier,
}

pub static SUPPORTED_BUILTIN_NODE_MODULES: &[NodeModulePolyfill] = &[
  NodeModulePolyfill {
    name: "assert",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/assert.ts"),
  },
  NodeModulePolyfill {
    name: "assert/strict",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/assert/strict.ts"),
  },
  NodeModulePolyfill {
    name: "async_hooks",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/async_hooks.ts"),
  },
  NodeModulePolyfill {
    name: "buffer",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/buffer.ts"),
  },
  NodeModulePolyfill {
    name: "child_process",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/child_process.ts"),
  },
  NodeModulePolyfill {
    name: "cluster",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/cluster.ts"),
  },
  NodeModulePolyfill {
    name: "console",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/console.ts"),
  },
  NodeModulePolyfill {
    name: "constants",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/constants.ts"),
  },
  NodeModulePolyfill {
    name: "crypto",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/crypto.ts"),
  },
  NodeModulePolyfill {
    name: "dgram",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/dgram.ts"),
  },
  NodeModulePolyfill {
    name: "dns",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/dns.ts"),
  },
  NodeModulePolyfill {
    name: "dns/promises",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/dns/promises.ts"),
  },
  NodeModulePolyfill {
    name: "domain",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/domain.ts"),
  },
  NodeModulePolyfill {
    name: "events",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/events.ts"),
  },
  NodeModulePolyfill {
    name: "fs",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/fs.ts"),
  },
  NodeModulePolyfill {
    name: "fs/promises",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/fs/promises.ts"),
  },
  NodeModulePolyfill {
    name: "http",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/http.ts"),
  },
  NodeModulePolyfill {
    name: "https",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/https.ts"),
  },
  NodeModulePolyfill {
    name: "module",
    specifier: NodeModulePolyfillSpecifier::Embedded(
      "internal:deno_node/module_es_shim.js",
    ),
  },
  NodeModulePolyfill {
    name: "net",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/net.ts"),
  },
  NodeModulePolyfill {
    name: "os",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/os.ts"),
  },
  NodeModulePolyfill {
    name: "path",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/path.ts"),
  },
  NodeModulePolyfill {
    name: "path/posix",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/path/posix.ts"),
  },
  NodeModulePolyfill {
    name: "path/win32",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/path/win32.ts"),
  },
  NodeModulePolyfill {
    name: "perf_hooks",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/perf_hooks.ts"),
  },
  NodeModulePolyfill {
    name: "process",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/process.ts"),
  },
  NodeModulePolyfill {
    name: "querystring",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/querystring.ts"),
  },
  NodeModulePolyfill {
    name: "readline",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/readline.ts"),
  },
  NodeModulePolyfill {
    name: "stream",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/stream.ts"),
  },
  NodeModulePolyfill {
    name: "stream/consumers",
    specifier: NodeModulePolyfillSpecifier::StdNode(
      "node/stream/consumers.mjs",
    ),
  },
  NodeModulePolyfill {
    name: "stream/promises",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/stream/promises.mjs"),
  },
  NodeModulePolyfill {
    name: "stream/web",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/stream/web.ts"),
  },
  NodeModulePolyfill {
    name: "string_decoder",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/string_decoder.ts"),
  },
  NodeModulePolyfill {
    name: "sys",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/sys.ts"),
  },
  NodeModulePolyfill {
    name: "timers",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/timers.ts"),
  },
  NodeModulePolyfill {
    name: "timers/promises",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/timers/promises.ts"),
  },
  NodeModulePolyfill {
    name: "tls",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/tls.ts"),
  },
  NodeModulePolyfill {
    name: "tty",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/tty.ts"),
  },
  NodeModulePolyfill {
    name: "url",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/url.ts"),
  },
  NodeModulePolyfill {
    name: "util",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/util.ts"),
  },
  NodeModulePolyfill {
    name: "util/types",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/util/types.ts"),
  },
  NodeModulePolyfill {
    name: "v8",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/v8.ts"),
  },
  NodeModulePolyfill {
    name: "vm",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/vm.ts"),
  },
  NodeModulePolyfill {
    name: "worker_threads",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/worker_threads.ts"),
  },
  NodeModulePolyfill {
    name: "zlib",
    specifier: NodeModulePolyfillSpecifier::StdNode("node/zlib.ts"),
  },
];
