//! Exercise typed durable task commands over local IPC.

use std::sync::{Arc, Mutex};

use muxyard_contracts::{
    Id, PROTOCOL_VERSION, Task, TaskAuditEvent, TaskAuditRequest, TaskAuditResponse,
    TaskCreateRequest, TaskListRequest, TaskListResponse, TaskState, TaskUpdateRequest,
};
use muxyard_ipc::{
    HostStatusResponse, IpcClient, IpcResponse, IpcServer, TASK_AUDIT_METHOD, TASK_CREATE_METHOD,
    TASK_LIST_METHOD, TASK_UPDATE_METHOD, TaskStore,
};
use muxyard_platform::PlatformPaths;

fn sample_status() -> HostStatusResponse {
    HostStatusResponse {
        host_version: "0.1.0".to_string(),
        protocol_version: PROTOCOL_VERSION,
        instance_id: "task-test".to_string(),
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
struct FakeTaskStore {
    tasks: Mutex<Vec<Task>>,
    audit: Mutex<Vec<TaskAuditEvent>>,
}

impl TaskStore for FakeTaskStore {
    fn list_tasks(&self, request: &TaskListRequest) -> Result<Vec<Task>, String> {
        Ok(self
            .tasks
            .lock()
            .unwrap()
            .iter()
            .filter(|task| {
                request
                    .project_id
                    .as_ref()
                    .is_none_or(|project| &task.project_id == project)
            })
            .cloned()
            .collect())
    }

    fn create_task(&self, request: &TaskCreateRequest) -> Result<Task, String> {
        let task = Task {
            id: Id("task-1".to_string()),
            project_id: request.project_id.clone(),
            title: request.title.clone(),
            goal: request.goal.clone(),
            constraints: request.constraints.clone(),
            state: TaskState::Draft,
            worktree_id: None,
            base_revision: request.base_revision.clone(),
            created_ms: 1,
            updated_ms: 1,
            state_version: 1,
        };
        self.tasks.lock().unwrap().push(task.clone());
        Ok(task)
    }

    fn update_task(&self, request: &TaskUpdateRequest) -> Result<Task, String> {
        let mut tasks = self.tasks.lock().unwrap();
        let task = tasks
            .iter_mut()
            .find(|task| task.id == request.task_id)
            .ok_or_else(|| "not found".to_string())?;
        if task.state_version != request.expected_version {
            return Err("conflict".to_string());
        }
        if let Some(title) = &request.title {
            task.title = title.clone();
        }
        if let Some(state) = request.state {
            task.state = state;
        }
        task.updated_ms += 1;
        task.state_version += 1;
        Ok(task.clone())
    }

    fn task_audit(&self, _task_id: &str) -> Result<Vec<TaskAuditEvent>, String> {
        Ok(self.audit.lock().unwrap().clone())
    }
}

#[test]
fn task_create_list_update_and_audit_roundtrip() {
    let (_temp, paths) = test_paths();
    let store = Arc::new(FakeTaskStore::default());
    let mut server = IpcServer::start_with_task_store(&paths, sample_status(), store).unwrap();
    let mut client = IpcClient::connect(&paths).unwrap();

    let created: IpcResponse<Task> = client
        .request(
            TASK_CREATE_METHOD,
            TaskCreateRequest {
                project_id: Id("project-1".to_string()),
                title: "Task".to_string(),
                goal: "Ship it".to_string(),
                constraints: vec!["no schema drift".to_string()],
                base_revision: Some("abc".to_string()),
            },
        )
        .unwrap();
    let task = match created {
        IpcResponse::Ok { data } => data,
        IpcResponse::Error { error } => panic!("task create failed: {error}"),
    };
    assert_eq!(task.state, TaskState::Draft);

    let listed: IpcResponse<TaskListResponse> = client
        .request(
            TASK_LIST_METHOD,
            TaskListRequest {
                project_id: Some(Id("project-1".to_string())),
            },
        )
        .unwrap();
    assert!(matches!(listed, IpcResponse::Ok { data } if data.tasks.len() == 1));

    let updated: IpcResponse<Task> = client
        .request(
            TASK_UPDATE_METHOD,
            TaskUpdateRequest {
                task_id: task.id.clone(),
                expected_version: 1,
                title: Some("Renamed".to_string()),
                goal: None,
                constraints: None,
                state: Some(TaskState::Ready),
            },
        )
        .unwrap();
    match updated {
        IpcResponse::Ok { data } => {
            assert_eq!(data.title, "Renamed");
            assert_eq!(data.state, TaskState::Ready);
            assert_eq!(data.state_version, 2);
        }
        IpcResponse::Error { error } => panic!("task update failed: {error}"),
    }

    let audit: IpcResponse<TaskAuditResponse> = client
        .request(
            TASK_AUDIT_METHOD,
            TaskAuditRequest {
                task_id: task.id.clone(),
            },
        )
        .unwrap();
    assert!(matches!(audit, IpcResponse::Ok { data } if data.events.is_empty()));

    drop(client);
    server.stop();
}
