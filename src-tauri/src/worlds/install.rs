//! The server JAR of a world version (PLAN W3, ADAPTATION A8).
//!
//! Mojang's version descriptor names the dedicated server in
//! `downloads.server`. The JAR goes through the same verified download executor
//! and host allowlist as client files, into `shared/servers/<sha1>/server.jar`,
//! and the world pins it by SHA-1, SHA-256 and size.
//!
//! A world with mods (W12) also needs its loader's server: the manifest of the
//! exact loader version the generation names, pinned in `world.json` by its
//! SHA-1, and each of its libraries in the Maven tree by the hashes the
//! loader's meta service or Maven repository publishes. The installer of a
//! Forge-style loader never runs; the launcher runs its server-side
//! processors itself and records every file they produce by its hash, beside
//! the manifest.

use std::io::Read;
use std::path::PathBuf;

use sha1::Digest as _;
use uuid::Uuid;

use super::world::{self, Mutation, MutationKind, ServerPin, WorldFile};
use super::{pack, tuple};
use crate::catalog::source::{Fetch, MetadataService};
use crate::download::{
    CancelToken, Digest, DownloadRequest, Executor, ProgressSink, resolve_managed_path,
};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::install::descriptor::{self, Download};
use crate::install::plan::{RuntimeRequirement, runtime_major, runtime_target};
use crate::install::rules;
use crate::install::rules::RuleContext;
use crate::install::service::PlanSource;
use crate::loader::manifest::{self, LoaderManifest, maven_path};
use crate::loader::processors;
use crate::net::SourcePolicy;
use crate::runtime::{InstalledRuntime, RuntimeInstaller};
use crate::storage::models::LockedArtifact;
use crate::storage::paths::{DataRoot, JavaProvider, Sha1, Sha256, WorldId, server_jar_relative};

/// What hosting a version needs to download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerPlan {
    /// The Java the version's descriptor names.
    pub java_major: u16,
    pub jar: Download,
    /// Mojang's server mappings, for a Forge-style loader's processors.
    pub mappings: Option<Download>,
    pub runtime: RuntimeRequirement,
}

/// Plans the server of a world version from its verified descriptor bytes,
/// with the Java the descriptor names.
pub fn plan(bytes: &[u8], minecraft: &str, ctx: &RuleContext) -> LauncherResult<ServerPlan> {
    if tuple::version(minecraft).is_none() {
        return Err(LauncherError::invalid_input(
            "Worlds are not available for this Minecraft version yet.",
        ));
    }
    let parsed = descriptor::parse(bytes, minecraft)?;
    let jar = parsed
        .server
        .ok_or_else(|| descriptor::unsupported("downloads.server"))?;
    let (os, arch) = runtime_target(ctx)?;
    Ok(ServerPlan {
        java_major: parsed.java_major,
        jar,
        mappings: parsed.server_mappings,
        runtime: RuntimeRequirement {
            major: runtime_major(parsed.java_major),
            os,
            arch,
            component: parsed.java_component,
        },
    })
}

/// The download of the JAR; its URL must be on Mojang's artifact hosts. Like
/// every Mojang file, it comes from the download mirror when Mojang fails. A
/// version of our own list has it on our update server only (D-073).
pub fn request(plan: &ServerPlan) -> LauncherResult<DownloadRequest> {
    let source = if SourcePolicy::ExtraVersions.check(&plan.jar.url).is_ok() {
        SourcePolicy::ExtraVersions
    } else {
        SourcePolicy::MojangArtifacts
    };
    Ok(DownloadRequest {
        source,
        url: source.check(&plan.jar.url)?,
        path: server_jar_relative(&plan.jar.sha1),
        size: plan.jar.size,
        digest: Digest::Sha1(plan.jar.sha1.clone()),
        label: Some("server.jar".to_owned()),
    })
}

/// Downloads (or reuses, after verification) the JAR and returns its pin.
pub async fn ensure(
    root: &DataRoot,
    executor: &Executor,
    plan: &ServerPlan,
    cancel: &CancelToken,
    sink: &dyn ProgressSink,
) -> LauncherResult<ServerPin> {
    executor
        .run(Uuid::new_v4(), vec![request(plan)?], cancel, sink)
        .await?;
    let (root, plan) = (root.clone(), plan.clone());
    tokio::task::spawn_blocking(move || pin(&root, &plan))
        .await
        .map_err(|_| LauncherError::new(ErrorCode::Internal, "The server check stopped."))?
}

/// Everything a world needs before its server can start: the server JAR,
/// pinned in `world.json`, and the version's Java runtime. Returns the world and
/// the runtime's Java executable.
#[allow(clippy::too_many_arguments)]
pub async fn prepare<F: Fetch>(
    root: &DataRoot,
    metadata: &MetadataService<F>,
    source: &dyn PlanSource,
    runtimes: &RuntimeInstaller,
    executor: &Executor,
    world_id: WorldId,
    java: JavaProvider,
    cancel: &CancelToken,
    sink: &dyn ProgressSink,
) -> LauncherResult<(WorldFile, PathBuf)> {
    let mut world = world::load(root, world_id)?;
    let (catalog, _, _, error) = metadata.catalog(false).await;
    let catalog = catalog.ok_or_else(|| {
        error.unwrap_or_else(|| {
            LauncherError::new(
                ErrorCode::NetworkError,
                "The version catalog is unavailable.",
            )
        })
    })?;
    let entry = catalog
        .find(&world.minecraft)
        .filter(|e| matches!(e.version_type.as_str(), "release" | "snapshot"))
        .ok_or_else(|| descriptor::unsupported("version not listed"))?;
    let bytes = metadata.descriptor(entry).await?;
    let plan = plan(&bytes, &world.minecraft, &RuleContext::current()?)?;

    let pinned = world
        .server
        .as_ref()
        .is_some_and(|p| p.jar_sha1 == plan.jar.sha1 && verify(root, p));
    if !pinned {
        world.mutation = Some(Mutation {
            kind: MutationKind::InstallServer,
            operation_id: Uuid::new_v4().to_string(),
            started_at: chrono::Utc::now(),
        });
        world::save(root, &world)?;
        let pin = ensure(root, executor, &plan, cancel, sink).await?;
        world.server = Some(pin);
        world.mutation = None;
        world::save(root, &world)?;
    }
    let package = source.runtime(&plan.runtime, java).await?;
    let runtime = runtimes
        .ensure(&package, Uuid::new_v4(), cancel, sink)
        .await?;
    ensure_loader(
        root,
        source,
        executor,
        &mut world,
        &Server {
            plan: &plan,
            runtime: &runtime,
            runtimes,
        },
        cancel,
        sink,
    )
    .await?;
    Ok((world, runtime.java))
}

/// The world's server and Java, for a loader's server-side processors.
struct Server<'a> {
    plan: &'a ServerPlan,
    runtime: &'a InstalledRuntime,
    /// Installs another Java when the world's is too old for the processors.
    runtimes: &'a RuntimeInstaller,
}

/// Where the files a loader's server-side processors produced are recorded.
fn server_files_path(root: &DataRoot, manifest: &Sha1) -> PathBuf {
    root.loader_manifest(manifest)
        .with_extension("server-files.json")
}

/// The recorded files of a loader's server, when all are intact.
fn server_files(root: &DataRoot, manifest: &Sha1) -> Option<Vec<LockedArtifact>> {
    let bytes = std::fs::read(server_files_path(root, manifest)).ok()?;
    let files: Vec<LockedArtifact> = serde_json::from_slice(&bytes).ok()?;
    processors::verify_generated(root, &files).then_some(files)
}

/// Mojang's server mappings, where a loader's processors read them.
fn mappings_path(plan: &ServerPlan) -> Option<(String, Download)> {
    let mappings = plan.mappings.clone()?;
    Some((
        format!("shared/servers/{}/server-mappings.txt", plan.jar.sha1),
        mappings,
    ))
}

/// Runs a Forge-style loader's server-side processors unless the files they
/// produce are recorded and intact.
#[allow(clippy::too_many_arguments)]
async fn ensure_processed(
    root: &DataRoot,
    source: &dyn PlanSource,
    executor: &Executor,
    loader: &LoaderManifest,
    sha1: &Sha1,
    server: &Server<'_>,
    cancel: &CancelToken,
    sink: &dyn ProgressSink,
) -> LauncherResult<()> {
    let Some(installer) = &loader.installer else {
        return Ok(());
    };
    if server_files(root, sha1).is_some() {
        return Ok(());
    }
    let mut requests = vec![DownloadRequest {
        source: SourcePolicy::for_artifact(&installer.url)?.0,
        url: SourcePolicy::for_artifact(&installer.url)?.1,
        path: maven_path(&installer.path),
        size: installer.size,
        digest: Digest::Sha1(installer.sha1.clone()),
        label: Some("installer".to_owned()),
    }];
    requests.extend(library_requests(&installer.libraries)?);
    let mappings = mappings_path(server.plan);
    if let Some((path, download)) = &mappings {
        let source = SourcePolicy::MojangArtifacts;
        requests.push(DownloadRequest {
            source,
            url: source.check(&download.url)?,
            path: path.clone(),
            size: download.size,
            digest: Digest::Sha1(download.sha1.clone()),
            label: Some("server mappings".to_owned()),
        });
    }
    executor.run(Uuid::new_v4(), requests, cancel, sink).await?;
    let java = crate::install::service::installer_java(
        source,
        server.runtimes,
        &server.plan.runtime,
        server.runtime,
        Uuid::new_v4(),
        cancel,
        sink,
        &|_line| {},
    )
    .await?;

    let (root, loader, sha1, cancel) = (root.clone(), loader.clone(), sha1.clone(), cancel.clone());
    let jar = root.server_jar(&server.plan.jar.sha1);
    let mappings = mappings.map(|(path, download)| (path, download.sha1));
    tokio::task::spawn_blocking(move || {
        // The tools' output is not kept: it names paths on this computer.
        let log = |_line: String| {};
        let files = processors::prepare_server(&processors::ServerInputs {
            root: &root,
            manifest: &loader,
            server_jar: jar,
            mappings,
            java: &java,
            work_dir: root.server_processing_dir(Uuid::new_v4()),
            cancel: &cancel,
            log: &log,
            quiet_limit: processors::QUIET_LIMIT,
        })?;
        let bytes = serde_json::to_vec(&files)
            .map_err(|_| LauncherError::internal("The server's files could not be recorded."))?;
        crate::storage::atomic::write(&server_files_path(&root, &sha1), &bytes)
            .map_err(|e| LauncherError::from_io(&e, "Saving mod loader files"))
    })
    .await
    .map_err(|_| LauncherError::internal("The mod loader installer stopped unexpectedly."))?
}

/// The loader's server of a world with mods: its manifest, resolved once and
/// pinned in the world, and every library downloaded and verified. A world
/// without mods needs nothing.
async fn ensure_loader(
    root: &DataRoot,
    source: &dyn PlanSource,
    executor: &Executor,
    world: &mut WorldFile,
    server: &Server<'_>,
    cancel: &CancelToken,
    sink: &dyn ProgressSink,
) -> LauncherResult<()> {
    let Some(generation) = world.pack.as_ref() else {
        return Ok(());
    };
    pack::check_supported(&generation.loader, &world.minecraft)?;
    let stored = generation.server_manifest.as_ref().and_then(|sha1| {
        manifest::load(root, sha1)
            .filter(|m| m.choice() == generation.loader && m.game_version == world.minecraft)
            .map(|m| (m, sha1.clone()))
    });
    let (loader, sha1) = match stored {
        Some(found) => found,
        None => {
            source
                .server_loader(&generation.loader, &world.minecraft)
                .await?
        }
    };
    executor
        .run(Uuid::new_v4(), loader_requests(&loader)?, cancel, sink)
        .await?;
    ensure_processed(root, source, executor, &loader, &sha1, server, cancel, sink).await?;
    if generation.server_manifest.as_ref() != Some(&sha1)
        && let Some(generation) = world.pack.as_mut()
    {
        generation.server_manifest = Some(sha1);
        world::save(root, world)?;
    }
    Ok(())
}

/// The libraries of a loader's server that apply on this computer. A
/// Forge-style installer's own files have no URL: it carries or builds them.
fn server_libraries(
    libraries: &[manifest::LoaderLibrary],
) -> LauncherResult<Vec<&manifest::LoaderLibrary>> {
    let ctx = RuleContext::current()?;
    let mut out = Vec::new();
    for library in libraries {
        if !library.url.is_empty() && rules::evaluate(library.rules.as_ref(), &ctx)? {
            out.push(library);
        }
    }
    Ok(out)
}

/// The downloads of a loader server's libraries; each must name a host on
/// the allowlist, a SHA-1 and a size.
pub fn loader_requests(loader: &LoaderManifest) -> LauncherResult<Vec<DownloadRequest>> {
    library_requests(&loader.libraries)
}

fn library_requests(libraries: &[manifest::LoaderLibrary]) -> LauncherResult<Vec<DownloadRequest>> {
    server_libraries(libraries)?
        .into_iter()
        .map(|library| {
            let (Some(sha1), Some(size)) = (&library.sha1, library.size) else {
                return Err(descriptor::unsupported("loader library without hash"));
            };
            let (source, url) = SourcePolicy::for_artifact(&library.url)?;
            Ok(DownloadRequest {
                source,
                url,
                path: maven_path(&library.path),
                size,
                digest: Digest::Sha1(sha1.clone()),
                label: Some(library.name.clone()),
            })
        })
        .collect()
}

/// How a world's loader server starts.
pub enum LoaderLaunch {
    /// Fabric and Quilt: a main class with its classpath.
    Classpath {
        main_class: String,
        classpath: Vec<PathBuf>,
    },
    /// A Forge-style loader: the installer's start arguments, main class
    /// included, with files placed in the Maven tree.
    Arguments(Vec<String>),
}

/// How a world's loader server starts, every library checked by its SHA-1
/// and size, and every file its processors produced by its record.
pub fn loader_launch(
    root: &DataRoot,
    sha1: &Sha1,
    game_jar: &std::path::Path,
) -> LauncherResult<LoaderLaunch> {
    let not_installed = || {
        LauncherError::new(
            ErrorCode::RepairRequired,
            "The world's mod loader is missing or changed. Host the world again to install it.",
        )
    };
    let loader = manifest::load(root, sha1).ok_or_else(not_installed)?;
    let (main_class, classpath) = loader_classpath(root, sha1)?;
    let Some(arguments) = &loader.server_arguments else {
        return Ok(LoaderLaunch::Classpath {
            main_class,
            classpath,
        });
    };
    // Every file the arguments name is a library checked by its pin, or a
    // file the processors produced, checked by its record.
    let produced = server_files(root, sha1).ok_or_else(not_installed)?;
    let installer = loader.installer.as_ref().map_or(&[][..], |i| &i.libraries);
    let mut named = Vec::new();
    let command = arguments.command(&root.maven_root(), game_jar, &mut |rest| {
        named.push(maven_path(rest));
    })?;
    for path in named {
        if produced.iter().any(|f| f.path == path) {
            continue;
        }
        let pinned = loader
            .libraries
            .iter()
            .chain(installer)
            .find(|l| !l.url.is_empty() && maven_path(&l.path) == path)
            .ok_or_else(not_installed)?;
        if !intact(root, pinned)? {
            return Err(not_installed());
        }
    }
    Ok(LoaderLaunch::Arguments(command))
}

/// Whether a pinned library's file matches its SHA-1 and size.
fn intact(root: &DataRoot, library: &manifest::LoaderLibrary) -> LauncherResult<bool> {
    let path = resolve_managed_path(root, &maven_path(&library.path))?;
    Ok(hash_file(&path).is_ok_and(|(size, file_sha1, _)| {
        Some(size) == library.size
            && library
                .sha1
                .as_ref()
                .is_some_and(|s| s.as_str() == file_sha1)
    }))
}

/// The main class and classpath of a world's loader server, every library
/// checked by its SHA-1 and size.
fn loader_classpath(root: &DataRoot, sha1: &Sha1) -> LauncherResult<(String, Vec<PathBuf>)> {
    let not_installed = || {
        LauncherError::new(
            ErrorCode::RepairRequired,
            "The world's mod loader is missing or changed. Host the world again to install it.",
        )
    };
    let loader = manifest::load(root, sha1).ok_or_else(not_installed)?;
    let mut paths = Vec::new();
    for library in server_libraries(&loader.libraries)? {
        if !intact(root, library)? {
            return Err(not_installed());
        }
        paths.push(resolve_managed_path(root, &maven_path(&library.path))?);
    }
    Ok((loader.main_class, paths))
}

/// Size, SHA-1 and SHA-256 of a file, read in bounded chunks.
fn hash_file(path: &std::path::Path) -> std::io::Result<(u64, String, String)> {
    let mut file = std::fs::File::open(path)?;
    let mut sha1 = sha1::Sha1::new();
    let mut sha256 = sha2::Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    let mut size = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        size += read as u64;
        sha1.update(&buffer[..read]);
        sha256.update(&buffer[..read]);
    }
    Ok((
        size,
        hex::encode(sha1.finalize()),
        hex::encode(sha256.finalize()),
    ))
}

/// Hashes the stored JAR; it must still match the plan's SHA-1 and size.
pub fn pin(root: &DataRoot, plan: &ServerPlan) -> LauncherResult<ServerPin> {
    let (size, sha1, sha256) = hash_file(&root.server_jar(&plan.jar.sha1))
        .map_err(|e| LauncherError::from_io(&e, "Checking the server"))?;
    if size != plan.jar.size || sha1 != plan.jar.sha1.as_str() {
        return Err(LauncherError::new(
            ErrorCode::HashMismatch,
            "The server file does not match Mojang's checksum.",
        ));
    }
    Ok(ServerPin {
        jar_sha1: plan.jar.sha1.clone(),
        jar_sha256: Sha256::parse(&sha256)?,
        jar_size: size,
        java_major: plan.java_major,
    })
}

/// Whether the pinned JAR is present and unchanged.
pub fn verify(root: &DataRoot, pin: &ServerPin) -> bool {
    hash_file(&root.server_jar(&pin.jar_sha1)).is_ok_and(|(size, sha1, sha256)| {
        size == pin.jar_size && sha1 == pin.jar_sha1.as_str() && sha256 == pin.jar_sha256.as_str()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::rules::MetaOs;
    use crate::storage::paths::{Sha1, TargetArch, TargetOs};

    const V1_21_1: &[u8] = include_bytes!("../../../tests/fixtures/mojang/1.21.1.json");
    const V26_3: &[u8] = include_bytes!("../../../tests/fixtures/mojang/26.3.json");

    fn ctx() -> RuleContext {
        RuleContext::with_default_features(MetaOs::Linux, "x86_64", None)
    }

    #[test]
    fn plans_the_official_server_of_1_21_1() {
        let plan = plan(V1_21_1, "1.21.1", &ctx()).unwrap();
        assert_eq!(
            plan.jar.sha1.as_str(),
            "59353fb40c36d304f2035d51e7d6e6baa98dc05c"
        );
        assert_eq!(plan.jar.size, 51_627_615);
        assert_eq!(plan.runtime.major, 21);
        assert_eq!(plan.runtime.os, TargetOs::Linux);
        assert_eq!(plan.runtime.arch, TargetArch::X64);
        assert_eq!(
            plan.runtime.component.as_deref(),
            Some("java-runtime-delta")
        );
        let request = request(&plan).unwrap();
        assert_eq!(
            request.path,
            "shared/servers/59353fb40c36d304f2035d51e7d6e6baa98dc05c/server.jar"
        );
        assert_eq!(request.url.host_str(), Some("piston-data.mojang.com"));
        assert!(request.source.mirrored());
    }

    #[test]
    fn a_version_of_our_list_has_its_server_from_our_server_only() {
        // 24w14potato as our list serves it (D-073).
        let data: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../data/extra-versions.json")).unwrap();
        let potato = data["versions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["id"] == "24w14potato")
            .unwrap();
        let bytes = potato["descriptor"].as_str().unwrap().as_bytes();
        // Its own id is not a world version yet; the toggle comes with Worlds.
        let mut descriptor: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        descriptor["id"] = "1.21.1".into();
        let plan = plan(&serde_json::to_vec(&descriptor).unwrap(), "1.21.1", &ctx()).unwrap();
        let request = request(&plan).unwrap();
        assert_eq!(request.source, SourcePolicy::ExtraVersions);
        assert_eq!(request.url.host_str(), Some("upd.lapi.sh"));
        assert!(!request.source.mirrored());
    }

    #[test]
    fn the_java_comes_from_the_descriptor() {
        let plan = plan(V26_3, "26.3", &ctx()).unwrap();
        assert_eq!((plan.java_major, plan.runtime.major), (25, 25));
        let mut old: serde_json::Value = serde_json::from_slice(V1_21_1).unwrap();
        old["id"] = "1.12.2".into();
        old["javaVersion"] = serde_json::json!({"component": "jre-legacy", "majorVersion": 8});
        let plan = super::plan(&serde_json::to_vec(&old).unwrap(), "1.12.2", &ctx()).unwrap();
        assert_eq!(plan.java_major, 8);
        assert_eq!(plan.runtime.component.as_deref(), Some("jre-legacy"));
    }

    #[test]
    fn only_world_versions_are_planned() {
        for id in ["1.7.2", "1.6.4", "14w02a", "1.21-pre0"] {
            assert!(plan(V1_21_1, id, &ctx()).is_err(), "{id}");
        }
    }

    fn with_server(server: serde_json::Value) -> Vec<u8> {
        let mut value: serde_json::Value = serde_json::from_slice(V1_21_1).unwrap();
        value["downloads"]["server"] = server;
        serde_json::to_vec(&value).unwrap()
    }

    #[test]
    fn foreign_or_malformed_server_entries_are_refused() {
        let foreign = with_server(serde_json::json!({
            "url": "https://example.com/server.jar",
            "sha1": "59353fb40c36d304f2035d51e7d6e6baa98dc05c",
            "size": 10
        }));
        let plan = plan(&foreign, "1.21.1", &ctx()).unwrap();
        assert!(request(&plan).is_err(), "host not on the allowlist");
        let missing_size = with_server(serde_json::json!({
            "url": "https://piston-data.mojang.com/server.jar",
            "sha1": "59353fb40c36d304f2035d51e7d6e6baa98dc05c"
        }));
        assert!(super::plan(&missing_size, "1.21.1", &ctx()).is_err());
        let mut value: serde_json::Value = serde_json::from_slice(V1_21_1).unwrap();
        value["downloads"].as_object_mut().unwrap().remove("server");
        let none = serde_json::to_vec(&value).unwrap();
        assert!(super::plan(&none, "1.21.1", &ctx()).is_err());
    }

    #[test]
    fn pin_checks_the_stored_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
        let body = b"not really a server";
        let sha1 = Sha1::parse(&hex::encode(sha1::Sha1::digest(body))).unwrap();
        let plan = ServerPlan {
            java_major: 21,
            jar: Download {
                url: "https://piston-data.mojang.com/x.jar".into(),
                sha1: sha1.clone(),
                size: body.len() as u64,
            },
            mappings: None,
            runtime: super::plan(V1_21_1, "1.21.1", &ctx()).unwrap().runtime,
        };
        assert!(pin(&root, &plan).is_err(), "missing");
        let path = root.server_jar(&sha1);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        let pinned = pin(&root, &plan).unwrap();
        assert_eq!(
            pinned.jar_sha256.as_str(),
            hex::encode(sha2::Sha256::digest(body))
        );
        assert!(verify(&root, &pinned));
        std::fs::write(&path, b"not really a serveR").unwrap();
        assert!(!verify(&root, &pinned));
        assert!(pin(&root, &plan).is_err());
    }
}
