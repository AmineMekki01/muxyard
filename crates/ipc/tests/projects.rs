//! Exercise typed project registration over local IPC.

use std::sync::{Arc, Mutex};

use muxyard_contracts::{
    ExecutionEnvironment, Id, PROTOCOL_VERSION, Project, ProjectListRequest, ProjectListResponse,
    ProjectRegisterRequest, ProjectRemoveRequest, ProjectRemoveResponse, ProjectTrust,
};
use muxyard_ipc::{
    HostStatusResponse, IpcClient, IpcResponse, IpcServer, PROJECT_LIST_METHOD,
    PROJECT_REGISTER_METHOD, PROJECT_REMOVE_METHOD, ProjectStore,
};
use muxyard_platform::PlatformPaths;

fn sample_status() -> HostStatusResponse {
    HostStatusResponse {
        host_version: "0.1.0".to_string(),
        protocol_version: PROTOCOL_VERSION,
        instance_id: "project-test".to_string(),
        pid: 42,
        process_start: "start-test".to_string(),
        started_ms: 1,
        ready: true,
    }
}

fn test_paths() -> (tempfile::TempDir, PlatformPaths) {
    let temp = tempfile::tempdir().unwrap();
    let paths = PlatformPaths::from_root(temp.path());
    paths.ensure().unwrap();
    (temp, paths)
}

#[derive(Default)]
struct FakeProjectStore {
    projects: Mutex<Vec<Project>>,
}

impl ProjectStore for FakeProjectStore {
    fn list_projects(&self, _request: &ProjectListRequest) -> Result<Vec<Project>, String> {
        Ok(self.projects.lock().unwrap().clone())
    }

    fn register_project(&self, request: &ProjectRegisterRequest) -> Result<Project, String> {
        let project = Project {
            id: Id("project-1".to_string()),
            name: request
                .name
                .clone()
                .unwrap_or_else(|| "project".to_string()),
            root_path: request.path.clone(),
            execution_environment: ExecutionEnvironment::NativeMac {},
            repo_id: None,
            trust: ProjectTrust::Untrusted,
            created_ms: 1,
            last_opened_ms: 1,
        };
        self.projects.lock().unwrap().push(project.clone());
        Ok(project)
    }

    fn remove_project(&self, request: &ProjectRemoveRequest) -> Result<bool, String> {
        let mut projects = self.projects.lock().unwrap();
        let before = projects.len();
        projects.retain(|project| project.id != request.id);
        Ok(projects.len() != before)
    }
}

#[test]
fn project_list_register_and_remove_roundtrip() {
    let (_temp, paths) = test_paths();
    let store = Arc::new(FakeProjectStore::default());
    let mut server = IpcServer::start_with_project_store(&paths, sample_status(), store).unwrap();
    let mut client = IpcClient::connect(&paths).unwrap();

    let empty: IpcResponse<ProjectListResponse> = client
        .request(PROJECT_LIST_METHOD, ProjectListRequest {})
        .unwrap();
    match empty {
        IpcResponse::Ok { data } => assert!(data.projects.is_empty()),
        IpcResponse::Error { error } => panic!("project list failed: {error}"),
    }

    let registered: IpcResponse<Project> = client
        .request(
            PROJECT_REGISTER_METHOD,
            ProjectRegisterRequest {
                path: "/tmp/project".to_string(),
                name: Some("Demo".to_string()),
            },
        )
        .unwrap();
    let project = match registered {
        IpcResponse::Ok { data } => data,
        IpcResponse::Error { error } => panic!("project register failed: {error}"),
    };
    assert_eq!(project.name, "Demo");
    assert_eq!(project.trust, ProjectTrust::Untrusted);

    let listed: IpcResponse<ProjectListResponse> = client
        .request(PROJECT_LIST_METHOD, ProjectListRequest {})
        .unwrap();
    match listed {
        IpcResponse::Ok { data } => assert_eq!(data.projects, vec![project.clone()]),
        IpcResponse::Error { error } => panic!("project list failed: {error}"),
    }

    let removed: IpcResponse<ProjectRemoveResponse> = client
        .request(
            PROJECT_REMOVE_METHOD,
            ProjectRemoveRequest {
                id: project.id.clone(),
            },
        )
        .unwrap();
    assert!(matches!(
        removed,
        IpcResponse::Ok { data } if data.removed
    ));

    drop(client);
    server.stop();
}
