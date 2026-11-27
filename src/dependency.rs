//! Bounded, provider-neutral Registry v2 dependency graph resolution.

use crate::{
    public_directory,
    registry::{self, LoadedRegistry},
    registry_v2, resolver,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::Path,
};

pub const MAX_NODES: usize = 1_000;
pub const MAX_EDGES: usize = 10_000;
pub const MAX_DEPTH: usize = 256;

pub type ItemKey = (String, String);

pub struct Snapshot {
    pub declared: String,
    pub identity: String,
    pub loaded: LoadedRegistry,
}

pub struct Node {
    pub snapshot: usize,
    pub dependencies: Vec<ItemKey>,
}

pub struct Graph {
    pub snapshots: Vec<Snapshot>,
    pub nodes: BTreeMap<ItemKey, Node>,
    /// Dependency-first deterministic order.
    pub order: Vec<ItemKey>,
    pub root: ItemKey,
}

impl Graph {
    pub fn resolve(
        root_declared: &str,
        root_loaded: LoadedRegistry,
        root_item_id: &str,
        project_config: &Path,
        global_config: &Path,
    ) -> Result<Self, String> {
        let public_root = matches!(
            root_loaded.source,
            registry::RegistrySource::PublicRemote { .. }
        );
        let directory = if public_root {
            Some(public_directory::PublicDirectory::fetch()?)
        } else {
            None
        };
        Self::resolve_using(
            root_declared,
            root_loaded,
            root_item_id,
            project_config,
            global_config,
            registry::load_registry,
            directory.as_ref(),
        )
    }

    fn resolve_using<F>(
        root_declared: &str,
        root_loaded: LoadedRegistry,
        root_item_id: &str,
        project_config: &Path,
        global_config: &Path,
        mut load: F,
        directory: Option<&public_directory::PublicDirectory>,
    ) -> Result<Self, String>
    where
        F: FnMut(&str) -> Result<LoadedRegistry, String>,
    {
        let root_v2 = root_loaded
            .v2
            .as_ref()
            .ok_or("Dependency graph roots must use Registry v2")?;
        let root_provider = format!("v2:{}", root_v2.id);
        let root = (root_provider.clone(), root_item_id.to_owned());
        root_v2
            .items
            .iter()
            .find(|item| item.id == root_item_id)
            .ok_or_else(|| {
                format!("Item {root_item_id} is missing from its Registry v2 snapshot")
            })?;

        let identity = registry::canonical_identity(root_declared)?;
        let mut snapshots = vec![Snapshot {
            declared: root_declared.to_owned(),
            identity: identity.clone(),
            loaded: root_loaded,
        }];
        let mut by_identity = HashMap::from([(identity, 0usize)]);
        let mut nodes = BTreeMap::new();
        let mut order = Vec::new();
        let mut visiting = Vec::<ItemKey>::new();
        let mut visited = BTreeSet::<ItemKey>::new();
        let mut edge_count = 0usize;

        resolve_node(
            &root,
            0,
            project_config,
            global_config,
            &mut snapshots,
            &mut by_identity,
            &mut nodes,
            &mut order,
            &mut visiting,
            &mut visited,
            &mut edge_count,
            &mut load,
            directory,
        )?;

        Ok(Self {
            snapshots,
            nodes,
            order,
            root,
        })
    }

    pub fn item(&self, key: &ItemKey) -> Result<&registry_v2::Item, String> {
        let node = self
            .nodes
            .get(key)
            .ok_or("Resolved dependency node is missing")?;
        let v2 = self.snapshots[node.snapshot]
            .loaded
            .v2
            .as_ref()
            .ok_or("Resolved dependency registry is not Registry v2")?;
        v2.items
            .iter()
            .find(|item| item.id == key.1)
            .ok_or_else(|| "Resolved dependency item is missing".to_string())
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_node(
    key: &ItemKey,
    snapshot_index: usize,
    project_config: &Path,
    global_config: &Path,
    snapshots: &mut Vec<Snapshot>,
    by_identity: &mut HashMap<String, usize>,
    nodes: &mut BTreeMap<ItemKey, Node>,
    order: &mut Vec<ItemKey>,
    visiting: &mut Vec<ItemKey>,
    visited: &mut BTreeSet<ItemKey>,
    edge_count: &mut usize,
    load: &mut impl FnMut(&str) -> Result<LoadedRegistry, String>,
    directory: Option<&public_directory::PublicDirectory>,
) -> Result<(), String> {
    if let Some(position) = visiting.iter().position(|entry| entry == key) {
        let mut cycle = visiting[position..]
            .iter()
            .map(display_key)
            .collect::<Vec<_>>();
        cycle.push(display_key(key));
        return Err(format!("Dependency cycle: {}", cycle.join(" -> ")));
    }
    if visited.contains(key) {
        return Ok(());
    }
    if visiting.len() >= MAX_DEPTH {
        return Err(format!(
            "Dependency graph exceeds maximum depth {MAX_DEPTH}"
        ));
    }
    if nodes.len().saturating_add(visiting.len()) >= MAX_NODES {
        return Err(format!(
            "Dependency graph exceeds maximum nodes {MAX_NODES}"
        ));
    }

    let item = snapshots[snapshot_index]
        .loaded
        .v2
        .as_ref()
        .and_then(|v2| v2.items.iter().find(|item| item.id == key.1))
        .ok_or_else(|| format!("Dependency item {} is missing", display_key(key)))?;
    let provider = snapshots[snapshot_index]
        .loaded
        .v2
        .as_ref()
        .ok_or("Dependency registry is not Registry v2")?
        .id
        .clone();
    if key.0 != format!("v2:{provider}") {
        return Err("Resolved dependency provider identity does not match its manifest".into());
    }
    let mut declarations: Vec<_> = item
        .dependencies
        .iter()
        .map(|dependency| {
            (
                dependency.provider_id.clone(),
                dependency.item_id.clone(),
                dependency.version.clone(),
                dependency.manifest_sha256.clone(),
            )
        })
        .collect();
    declarations.sort();
    *edge_count = edge_count
        .checked_add(declarations.len())
        .ok_or("Dependency edge count overflow")?;
    if *edge_count > MAX_EDGES {
        return Err(format!(
            "Dependency graph exceeds maximum edges {MAX_EDGES}"
        ));
    }

    visiting.push(key.clone());
    let mut edges = Vec::with_capacity(declarations.len());
    for (dependency_provider, dependency_item_id, dependency_version, dependency_digest) in
        declarations
    {
        let dependency_snapshot = if dependency_provider == provider {
            if dependency_digest.is_some() {
                return Err("Internal dependency unexpectedly pins another manifest".into());
            }
            snapshot_index
        } else {
            let pinned_digest = dependency_digest
                .as_deref()
                .ok_or("External dependency has no pinned manifest digest")?;
            select_external_snapshot(
                &dependency_provider,
                pinned_digest,
                project_config,
                global_config,
                snapshots,
                by_identity,
                load,
                directory,
            )?
        };
        let dependency_v2 = snapshots[dependency_snapshot]
            .loaded
            .v2
            .as_ref()
            .ok_or("Dependency registry is not Registry v2")?;
        let dependency_item = dependency_v2
            .items
            .iter()
            .find(|candidate| candidate.id == dependency_item_id)
            .ok_or_else(|| {
                format!(
                    "Missing dependency {}/{}",
                    dependency_provider, dependency_item_id
                )
            })?;
        if dependency_item.version != dependency_version {
            return Err(format!(
                "Dependency {}/{} requires version {}, but the pinned snapshot contains {}",
                dependency_provider,
                dependency_item_id,
                dependency_version,
                dependency_item.version
            ));
        }
        let resolved_key = (
            format!("v2:{}", dependency_v2.id),
            dependency_item_id.clone(),
        );
        if let Some(previous) = nodes.get(&resolved_key)
            && (previous.snapshot != dependency_snapshot
                || snapshots[previous.snapshot].loaded.manifest_sha256
                    != snapshots[dependency_snapshot].loaded.manifest_sha256)
        {
            return Err(format!(
                "Conflicting snapshots selected for dependency {}",
                display_key(&resolved_key)
            ));
        }
        resolve_node(
            &resolved_key,
            dependency_snapshot,
            project_config,
            global_config,
            snapshots,
            by_identity,
            nodes,
            order,
            visiting,
            visited,
            edge_count,
            load,
            directory,
        )?;
        edges.push(resolved_key);
    }
    visiting.pop();
    nodes.insert(
        key.clone(),
        Node {
            snapshot: snapshot_index,
            dependencies: edges,
        },
    );
    visited.insert(key.clone());
    order.push(key.clone());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn select_external_snapshot(
    provider_id: &str,
    manifest_sha256: &str,
    project_config: &Path,
    global_config: &Path,
    snapshots: &mut Vec<Snapshot>,
    by_identity: &mut HashMap<String, usize>,
    load: &mut impl FnMut(&str) -> Result<LoadedRegistry, String>,
    directory: Option<&public_directory::PublicDirectory>,
) -> Result<usize, String> {
    let configured = resolver::effective_registries(project_config, global_config)?;
    let mut candidates = BTreeMap::<String, usize>::new();
    let mut seen_source = BTreeSet::new();
    let mut found_provider = false;
    for (alias, location) in configured {
        let identity = registry::canonical_identity(&location).map_err(|error| {
            format!("Cannot resolve configured registry alias {alias}: {error}")
        })?;
        if !seen_source.insert(identity.clone()) {
            continue;
        }
        let index = if let Some(index) = by_identity.get(&identity).copied() {
            index
        } else {
            let loaded = load(&location)
                .map_err(|error| format!("Cannot validate configured registry {alias}: {error}"))?;
            let index = snapshots.len();
            snapshots.push(Snapshot {
                declared: location,
                identity: identity.clone(),
                loaded,
            });
            by_identity.insert(identity, index);
            index
        };
        let snapshot = &snapshots[index].loaded;
        if snapshot.v2.as_ref().is_some_and(|v2| v2.id == provider_id) {
            found_provider = true;
            if snapshot.manifest_sha256 == manifest_sha256 {
                candidates.insert(snapshots[index].identity.clone(), index);
            }
        }
    }
    if candidates.is_empty()
        && let Some(directory) = directory
    {
        let entry = directory.get(provider_id.strip_prefix("v2:").unwrap_or(provider_id));
        if let Some(entry) = entry.filter(|entry| entry.status == public_directory::Status::Active)
        {
            if entry.manifest_sha256 != manifest_sha256 {
                return Err(format!(
                    "Stale public provider snapshot for {provider_id}: directory digest does not match required digest {manifest_sha256}"
                ));
            }
            let loaded = public_directory::PublicDirectory::load_manifest(entry)?;
            let identity = registry::canonical_identity(&entry.registry_url)?;
            if let Some(index) = by_identity.get(&identity).copied() {
                return Ok(index);
            }
            let index = snapshots.len();
            snapshots.push(Snapshot {
                declared: entry.registry_url.clone(),
                identity: identity.clone(),
                loaded,
            });
            by_identity.insert(identity, index);
            return Ok(index);
        }
    }
    match candidates.len() {
        0 if found_provider => Err(format!(
            "Stale provider snapshot for {provider_id}: no configured manifest matches required digest {manifest_sha256}"
        )),
        0 => Err(format!(
            "Missing provider snapshot {provider_id} at manifest digest {manifest_sha256}; register the exact reviewed registry snapshot"
        )),
        1 => Ok(*candidates.values().next().expect("one candidate")),
        _ => Err(format!(
            "Ambiguous provider snapshot {provider_id} at manifest digest {manifest_sha256}; configured aliases resolve to different manifest locations"
        )),
    }
}

fn display_key(key: &ItemKey) -> String {
    format!("{}/{}", key.0, key.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::{
        ServerConfig, ServerConnection, StreamOwned,
        pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer},
    };
    use sha2::Digest;
    use std::{
        fs,
        io::{Read, Write},
        net::TcpListener,
        sync::Arc,
        thread,
    };

    #[test]
    fn resolves_external_dependency_from_a_trusted_https_registry_fixture() {
        let external = serde_json::json!({
            "schemaVersion": 2,
            "id": "org.example.assets",
            "name": "assets",
            "items": [{
                "id": "asset", "version": "1.0.0", "kind": "asset",
                "files": [{"source":"asset.bin", "target":"asset.bin", "sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "size":1}]
            }]
        })
        .to_string();
        let digest = format!("{:x}", sha2::Sha256::digest(external.as_bytes()));
        let root_dir = tempfile::tempdir().unwrap();
        let root_manifest = root_dir.path().join("root.json");
        fs::write(
            &root_manifest,
            serde_json::json!({
                "schemaVersion": 2, "id": "org.example.components", "name": "components",
                "items": [{
                    "id":"root", "version":"1.0.0", "kind":"component",
                    "files":[{"source":"root.bin", "target":"root.bin", "sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "size":1}],
                    "dependencies":[{"providerId":"org.example.assets", "itemId":"asset", "version":"1.0.0", "manifestSha256":digest}]
                }]
            })
            .to_string(),
        )
        .unwrap();

        let generated = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let root_certificate =
            reqwest::Certificate::from_der(generated.cert.der().as_ref()).unwrap();
        let private_key =
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(generated.key_pair.serialize_der()));
        let server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![generated.cert.der().clone()], private_key)
            .unwrap();
        let listener = TcpListener::bind("localhost:0").unwrap();
        let url = format!(
            "https://localhost:{}/registry.json",
            listener.local_addr().unwrap().port()
        );
        let body = external.into_bytes();
        let server_config = Arc::new(server_config);
        let server = thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            let mut stream =
                StreamOwned::new(ServerConnection::new(server_config).unwrap(), socket);
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).unwrap();
                assert_ne!(count, 0);
                request.extend_from_slice(&buffer[..count]);
            }
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
            stream.conn.send_close_notify();
            stream.flush().unwrap();
        });

        let project = root_dir.path().join(".friladd.json");
        let global = root_dir.path().join("global.json");
        fs::write(
            &project,
            serde_json::json!({
                "schemaVersion": 1,
                "registries": {"assets": {"source": url}}
            })
            .to_string(),
        )
        .unwrap();
        let root_loaded = registry::load_registry(root_manifest.to_str().unwrap()).unwrap();
        let graph = Graph::resolve_using(
            root_manifest.to_str().unwrap(),
            root_loaded,
            "root",
            &project,
            &global,
            |location| {
                if location.starts_with("https://localhost:") {
                    registry::load_registry_with_test_root(location, root_certificate.clone())
                } else {
                    registry::load_registry(location)
                }
            },
            None,
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(
            graph.order,
            vec![
                ("v2:org.example.assets".into(), "asset".into()),
                ("v2:org.example.components".into(), "root".into()),
            ]
        );
        assert_eq!(graph.item(&graph.order[0]).unwrap().version, "1.0.0");
    }
}
