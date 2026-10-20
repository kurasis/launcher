//! The server JAR of a world version (PLAN W3, ADAPTATION A8).
//!
//! Mojang's version descriptor names the dedicated server in
//! `downloads.server`. The JAR goes through the same verified download executor
//! and host allowlist as client files, into `shared/servers/<sha1>/server.jar`,
//! and the world pins it by SHA-1, SHA-256 and size.

use std::io::Read;
use std::path::PathBuf;

use sha1::Digest as _;
use uuid::Uuid;

use super::tuple;
use super::world::{self, Mutation, MutationKind, ServerPin, WorldFile};
use crate::catalog::source::{Fetch, MetadataService};
use crate::download::{CancelToken, Digest, DownloadRequest, Executor, ProgressSink};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::install::descriptor::{self, Download};
use crate::install::plan::{RuntimeRequirement, runtime_major, runtime_target};
use crate::install::rules::RuleContext;
use crate::install::service::PlanSource;
use crate::net::SourcePolicy;
use crate::runtime::RuntimeInstaller;
use crate::storage::paths::{DataRoot, JavaProvider, Sha256, WorldId, server_jar_relative};

/// What hosting a version needs to download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerPlan {
    /// The Java the version's descriptor names.
    pub java_major: u16,
    pub jar: Download,
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
        runtime: RuntimeRequirement {
            major: runtime_major(parsed.java_major),
            os,
            arch,
            component: parsed.java_component,
        },
    })
}

/// The download of the JAR; its URL must be on Mojang's artifact hosts. Like
/// every Mojang file, it comes from the download mirror when Mojang fails.
pub fn request(plan: &ServerPlan) -> LauncherResult<DownloadRequest> {
    let source = SourcePolicy::MojangArtifacts;
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
    Ok((world, runtime.java))
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
