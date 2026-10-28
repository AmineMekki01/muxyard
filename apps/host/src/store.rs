//! Host-owned terminal-session metadata and workspace layout persistence.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use muxyard_contracts::{
    ExecutionEnvironment, Id, MAX_WORKSPACE_LAYOUT_BYTES, Project, ProjectListRequest,
    ProjectRegisterRequest, ProjectRemoveRequest, ProjectTrust, TerminalLayoutNode,
    WorkspaceLayout, WorkspaceLayoutSnapshot,
};
use muxyard_git::discover_repository;
use muxyard_ipc::{ProjectStore, WorkspaceLayoutStore};
use muxyard_storage::{
    Database, ProjectTrust as StoredProjectTrust, TerminalSessionState, delete_workspace_layout,
    get_workspace_layout, list_projects, record_terminal_session, register_project, remove_project,
    save_workspace_layout, update_terminal_session_state,
};
use muxyard_terminal::{PtySessionIdentity, TerminalSessionStore, TerminalStoreError};

use crate::now_ms;

/// Shared SQLite-backed host store.
pub struct HostStore {
    database: Arc<Mutex<Database>>,
    host_instance_id: String,
    process_start: String,
    execution_environment_id: &'static str,
}

impl HostStore {
    /// Build a store for the current host process.
    pub fn new(
        database: Arc<Mutex<Database>>,
        host_instance_id: String,
        process_start: String,
    ) -> Self {
        Self {
            database,
            host_instance_id,
            process_start,
            execution_environment_id: execution_environment_id(),
        }
    }

    fn database(&self) -> std::sync::MutexGuard<'_, Database> {
        self.database
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl TerminalSessionStore for HostStore {
    fn record_spawned(&self, identity: &PtySessionIdentity) -> Result<(), TerminalStoreError> {
        let database = self.database();
        record_terminal_session(
            database.connection(),
            &identity.stream_id.to_string(),
            &self.host_instance_id,
            self.execution_environment_id,
            &identity.cwd.to_string_lossy(),
            &identity.shell.to_string_lossy(),
            now_ms(),
        )
        .map_err(|error| TerminalStoreError(error.to_string()))
    }

    fn record_closed(&self, stream_id: uuid::Uuid, state: &str) -> Result<(), TerminalStoreError> {
        let state = match state {
            "exited" => TerminalSessionState::Exited,
            "lost" => TerminalSessionState::Lost,
            _ => TerminalSessionState::Running,
        };
        let database = self.database();
        update_terminal_session_state(
            database.connection(),
            &stream_id.to_string(),
            state,
            now_ms(),
        )
        .map(|_| ())
        .map_err(|error| TerminalStoreError(error.to_string()))
    }
}

impl ProjectStore for HostStore {
    fn list_projects(&self, _request: &ProjectListRequest) -> Result<Vec<Project>, String> {
        let database = self.database();
        list_projects(database.connection())
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|project| {
                Ok(Project {
                    id: Id(project.id),
                    name: project.name,
                    root_path: project.root_path,
                    execution_environment: execution_environment_from_id(
                        &project.execution_environment_id,
                    )?,
                    repo_id: project.repo_id.map(Id),
                    trust: match project.trust {
                        StoredProjectTrust::Untrusted => ProjectTrust::Untrusted,
                        StoredProjectTrust::Trusted => ProjectTrust::Trusted,
                    },
                    created_ms: project.created_ms,
                    last_opened_ms: project.last_opened_ms,
                })
            })
            .collect()
    }

    fn register_project(&self, request: &ProjectRegisterRequest) -> Result<Project, String> {
        let discovery = discover_repository(&request.path).map_err(|error| error.to_string())?;
        let name = request
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .or_else(|| {
                discovery
                    .root
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "Project".to_string());
        let database = self.database();
        let stored = register_project(
            database.connection(),
            &uuid::Uuid::new_v4().to_string(),
            &name,
            &discovery.root.to_string_lossy(),
            self.execution_environment_id,
            None,
            now_ms(),
        )
        .map_err(|error| error.to_string())?;
        Ok(Project {
            id: Id(stored.id),
            name: stored.name,
            root_path: stored.root_path,
            execution_environment: execution_environment_from_id(&stored.execution_environment_id)?,
            repo_id: stored.repo_id.map(Id),
            trust: match stored.trust {
                StoredProjectTrust::Untrusted => ProjectTrust::Untrusted,
                StoredProjectTrust::Trusted => ProjectTrust::Trusted,
            },
            created_ms: stored.created_ms,
            last_opened_ms: stored.last_opened_ms,
        })
    }

    fn remove_project(&self, request: &ProjectRemoveRequest) -> Result<bool, String> {
        let database = self.database();
        remove_project(database.connection(), &request.id.0).map_err(|error| error.to_string())
    }
}

impl WorkspaceLayoutStore for HostStore {
    fn load_layout(&self) -> Result<WorkspaceLayoutSnapshot, String> {
        let database = self.database();
        let stored = get_workspace_layout(database.connection(), "default")
            .map_err(|error| error.to_string())?;
        let Some(stored) = stored else {
            return Ok(self.snapshot(None, None, None));
        };
        let parsed = serde_json::from_str::<WorkspaceLayout>(&stored.structure_json)
            .ok()
            .filter(validate_layout);
        if parsed.is_none() {
            let _ = delete_workspace_layout(database.connection(), "default");
        }
        Ok(self.snapshot(parsed, stored.host_instance_id, stored.process_start_token))
    }

    fn save_layout(&self, layout: &WorkspaceLayout) -> Result<(), String> {
        if !validate_layout(layout) {
            return Err("workspace layout is invalid".to_string());
        }
        let encoded = serde_json::to_string(layout).map_err(|error| error.to_string())?;
        if encoded.len() > MAX_WORKSPACE_LAYOUT_BYTES {
            return Err("workspace layout is too large".to_string());
        }
        let database = self.database();
        save_workspace_layout(
            database.connection(),
            "default",
            &encoded,
            &self.host_instance_id,
            &self.process_start,
            now_ms(),
        )
        .map_err(|error| error.to_string())
    }
}

impl HostStore {
    fn snapshot(
        &self,
        layout: Option<WorkspaceLayout>,
        layout_host_instance_id: Option<String>,
        layout_process_start: Option<String>,
    ) -> WorkspaceLayoutSnapshot {
        WorkspaceLayoutSnapshot {
            layout,
            host_instance_id: self.host_instance_id.clone(),
            process_start: self.process_start.clone(),
            layout_host_instance_id,
            layout_process_start,
        }
    }
}

fn validate_layout(layout: &WorkspaceLayout) -> bool {
    if layout.version != 1 || layout.tabs.len() > 64 {
        return false;
    }
    let mut tab_ids = HashSet::new();
    let mut pane_ids = HashSet::new();
    let mut active_tab_found = layout.active_tab_id.is_none();
    for tab in &layout.tabs {
        if !tab_ids.insert(tab.id.0.clone()) {
            return false;
        }
        if layout.active_tab_id.as_ref() == Some(&tab.id) {
            active_tab_found = true;
        }
        let mut tab_panes = Vec::new();
        if !validate_node(&tab.root, 0, &mut tab_panes) {
            return false;
        }
        if tab_panes.len() > 128 || !tab_panes.iter().any(|id| id == &tab.active_pane_id.0) {
            return false;
        }
        if let Some(zoomed) = &tab.zoomed_pane_id
            && !tab_panes.iter().any(|id| id == &zoomed.0)
        {
            return false;
        }
        for id in tab_panes {
            if !pane_ids.insert(id) {
                return false;
            }
        }
    }
    active_tab_found
}

fn validate_node(node: &TerminalLayoutNode, depth: usize, panes: &mut Vec<String>) -> bool {
    if depth > 32 {
        return false;
    }
    match node {
        TerminalLayoutNode::Leaf {
            id,
            history_id,
            session_id,
            ..
        } => {
            if id.0.is_empty() || history_id.0.is_empty() {
                return false;
            }
            if session_id
                .as_ref()
                .is_some_and(|session| session.0.is_empty())
            {
                return false;
            }
            panes.push(id.0.clone());
            true
        }
        TerminalLayoutNode::Split {
            ratio,
            first,
            second,
            ..
        } => {
            (0.15..=0.85).contains(ratio)
                && validate_node(first, depth + 1, panes)
                && validate_node(second, depth + 1, panes)
        }
    }
}

fn execution_environment_from_id(id: &str) -> Result<ExecutionEnvironment, String> {
    match id {
        "native-mac" => Ok(ExecutionEnvironment::NativeMac {}),
        "native-linux" => Ok(ExecutionEnvironment::NativeLinux {}),
        "native-windows" => Ok(ExecutionEnvironment::NativeWindows {}),
        _ => Err(format!("unsupported project execution environment: {id}")),
    }
}

fn execution_environment_id() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "native-mac"
    }
    #[cfg(target_os = "linux")]
    {
        "native-linux"
    }
    #[cfg(target_os = "windows")]
    {
        "native-windows"
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        "native-unknown"
    }
}
