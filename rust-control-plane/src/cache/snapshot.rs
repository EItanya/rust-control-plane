use crate::cache::{Cache, FetchError, KnownResourceNames, WatchId, WatchResponder};
use crate::snapshot::{Resources, Snapshot};
use async_trait::async_trait;
use data_plane_api::envoy::config::core::v3::Node;
use data_plane_api::envoy::service::discovery::v3::{DiscoveryRequest, DiscoveryResponse};
use slab::Slab;
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::info;

#[derive(Debug)]
pub struct SnapshotCache {
    inner: Mutex<Inner>,
    ads: bool,
}

#[derive(Debug)]
struct Inner {
    status: HashMap<String, NodeStatus>,
    snapshots: HashMap<String, Snapshot>,
}

#[derive(Debug)]
struct NodeStatus {
    last_request_time: Instant,
    watches: Slab<Watch>,
}

impl NodeStatus {
    fn new() -> Self {
        Self {
            last_request_time: Instant::now(),
            watches: Slab::new(),
        }
    }
}

#[derive(Debug)]
struct Watch {
    req: DiscoveryRequest,
    tx: WatchResponder,
}

impl SnapshotCache {
    pub fn new(ads: bool) -> Self {
        Self {
            inner: Mutex::new(Inner::new()),
            ads,
        }
    }

    // Updates snapshot associated with a given node so that future requests receive it.
    // Triggers existing watches for the given node.
    pub async fn set_snapshot(&self, node: &str, snapshot: Snapshot) {
        let mut inner = self.inner.lock().await;
        inner.snapshots.insert(node.to_string(), snapshot.clone());
        if let Some(status) = inner.status.get_mut(node) {
            let mut to_delete = Vec::new();
            for (watch_id, watch) in &mut status.watches {
                let version = snapshot.version(&watch.req.type_url);
                if version != watch.req.version_info {
                    to_delete.push(watch_id)
                }
            }

            for watch_id in to_delete {
                let watch = status.watches.remove(watch_id);
                let resources = snapshot.resources(&watch.req.type_url);
                let version = snapshot.version(&watch.req.type_url);
                info!(
                    "watch triggered version={} type_url={}",
                    version, &watch.req.type_url
                );
                respond(&watch.req, watch.tx, resources, version).await;
            }
        }
    }

    pub async fn node_status(&self) -> HashMap<String, Instant> {
        let inner = self.inner.lock().await;
        inner
            .status
            .iter()
            .map(|(k, v)| (k.clone(), v.last_request_time))
            .collect()
    }
}

#[async_trait]
impl Cache for SnapshotCache {
    // Either responds on tx immediately, or sets a watch, returning a watch ID.
    async fn create_watch(
        &self,
        req: &DiscoveryRequest,
        tx: WatchResponder,
        known_resource_names: &KnownResourceNames,
    ) -> Option<WatchId> {
        let mut inner = self.inner.lock().await;
        let node_id = hash_id(&req.node);
        inner.update_node_status(&node_id);
        if let Some(snapshot) = inner.snapshots.get(&node_id) {
            let resources = snapshot.resources(&req.type_url);
            let version = snapshot.version(&req.type_url);
            let type_known_resource_names = known_resource_names.get(&req.type_url);
            // Check if a different set of resources has been requested.
            if inner.is_requesting_new_resources(req, resources, type_known_resource_names) {
                if self.ads && check_ads_consistency(req, resources) {
                    info!("not responding: ads consistency");
                    return Some(inner.set_watch(&node_id, req, tx));
                }
                info!("responding: resource diff");
                // TODO: Don't hold lock across await boundaries (performance).
                respond(req, tx, resources, version).await;
                return None;
            }
            if req.version_info == version {
                // Client is already at the latest version, so we have nothing to respond with.
                // Set a watch because we may receive a new version in the future.
                info!("set watch: latest version");
                Some(inner.set_watch(&node_id, req, tx))
            } else {
                // The version has changed, so we should respond.
                if self.ads && check_ads_consistency(req, resources) {
                    info!("not responding: ads consistency");
                    return Some(inner.set_watch(&node_id, req, tx));
                }
                info!("responding: new version");
                // TODO: Don't hold lock across await boundaries (performance).
                respond(req, tx, resources, version).await;
                None
            }
        } else {
            // No snapshot exists for this node, so we have nothing to respond with.
            // Set a watch because we may receive a snapshot for this node in the future.
            info!("set watch: no snapshot");
            Some(inner.set_watch(&node_id, req, tx))
        }
    }

    // Deletes a watch previously created with create_watch.
    async fn cancel_watch(&self, watch_id: &WatchId) {
        let mut inner = self.inner.lock().await;
        if let Some(status) = inner.status.get_mut(&watch_id.node_id) {
            status.watches.try_remove(watch_id.index);
        }
    }

    async fn fetch<'a>(
        &'a self,
        req: &'a DiscoveryRequest,
        type_url: &'static str,
    ) -> Result<DiscoveryResponse, FetchError> {
        let inner = self.inner.lock().await;
        let node_id = hash_id(&req.node);
        let snapshot = inner.snapshots.get(&node_id).ok_or(FetchError::NotFound)?;
        let version = snapshot.version(&req.type_url);
        if req.version_info == version {
            return Err(FetchError::VersionUpToDate);
        }
        let resources = snapshot.resources(type_url);
        Ok(build_response(req, resources, version))
    }
}

impl Inner {
    fn new() -> Self {
        Self {
            status: HashMap::new(),
            snapshots: HashMap::new(),
        }
    }

    fn set_watch(&mut self, node_id: &str, req: &DiscoveryRequest, tx: WatchResponder) -> WatchId {
        let watch = Watch {
            req: req.clone(),
            tx,
        };
        let status = self.status.get_mut(node_id).unwrap();
        let index = status.watches.insert(watch);
        WatchId {
            node_id: node_id.to_string(),
            index,
        }
    }

    fn update_node_status(&mut self, node_id: &str) {
        self.status
            .entry(node_id.to_string())
            .and_modify(|entry| entry.last_request_time = Instant::now())
            .or_insert_with(NodeStatus::new);
    }

    fn is_requesting_new_resources(
        &self,
        req: &DiscoveryRequest,
        resources: Option<&Resources>,
        type_known_resource_names: Option<&HashSet<String>>,
    ) -> bool {
        if let Some(resources) = resources {
            if let Some(known_resource_names) = type_known_resource_names {
                let mut diff = Vec::new();
                for name in &req.resource_names {
                    if !known_resource_names.contains(name) {
                        diff.push(name)
                    }
                }
                for name in diff {
                    if resources.items.contains_key(name) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

fn hash_id(node: &Option<Node>) -> String {
    node.as_ref().map_or(String::new(), |node| node.id.clone())
}

fn build_response(
    req: &DiscoveryRequest,
    resources: Option<&Resources>,
    version: &str,
) -> DiscoveryResponse {
    let mut filtered_resources = Vec::new();
    if let Some(resources) = resources {
        if req.resource_names.is_empty() {
            filtered_resources = resources
                .items
                .values()
                .map(|resource| resource.into_any())
                .collect();
        } else {
            for name in &req.resource_names {
                if let Some(resource) = resources.items.get(name) {
                    filtered_resources.push(resource.into_any())
                }
            }
        }
    }
    DiscoveryResponse {
        type_url: req.type_url.clone(),
        nonce: String::new(),
        version_info: version.to_string(),
        resources: filtered_resources,
        control_plane: None,
        canary: false,
    }
}

async fn respond(
    req: &DiscoveryRequest,
    tx: WatchResponder,
    resources: Option<&Resources>,
    version: &str,
) {
    let rep = build_response(req, resources, version);
    tx.send((req.clone(), rep)).await.unwrap();
}

fn check_ads_consistency(req: &DiscoveryRequest, resources: Option<&Resources>) -> bool {
    if !req.resource_names.is_empty() {
        if let Some(resources) = resources {
            let set: HashSet<&String> = HashSet::from_iter(req.resource_names.iter());
            for (name, _) in resources.items.iter() {
                if !set.contains(name) {
                    return false;
                }
            }
        }
    }
    true
}
