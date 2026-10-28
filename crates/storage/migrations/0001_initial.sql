CREATE TABLE host_instances (
    id TEXT PRIMARY KEY,
    pid INTEGER NOT NULL,
    os_user TEXT NOT NULL,
    process_start_token TEXT NOT NULL,
    host_version TEXT NOT NULL,
    protocol_version INTEGER NOT NULL,
    state TEXT NOT NULL CHECK (state IN (
        'starting', 'migrating_db', 'binding_local_api',
        'ready', 'draining', 'stopped', 'failed'
    )),
    started_ms INTEGER NOT NULL,
    heartbeat_ms INTEGER NOT NULL
);

CREATE INDEX idx_host_instances_heartbeat ON host_instances(heartbeat_ms DESC);

CREATE TABLE projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    root_path TEXT NOT NULL,
    execution_environment_id TEXT NOT NULL,
    repo_id TEXT,
    trust_state TEXT NOT NULL DEFAULT 'untrusted'
        CHECK (trust_state IN ('untrusted', 'trusted')),
    created_ms INTEGER NOT NULL,
    last_opened_ms INTEGER NOT NULL,
    UNIQUE (root_path, execution_environment_id)
);

CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    title TEXT NOT NULL,
    goal TEXT NOT NULL DEFAULT '',
    constraints_json TEXT NOT NULL DEFAULT '[]',
    state TEXT NOT NULL CHECK (state IN (
        'draft', 'ready', 'running', 'blocked', 'review',
        'approved', 'integrated', 'failed', 'canceled'
    )),
    worktree_id TEXT,
    base_revision TEXT,
    created_ms INTEGER NOT NULL,
    updated_ms INTEGER NOT NULL,
    state_version INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX idx_tasks_project_state ON tasks(project_id, state, updated_ms DESC);

CREATE TABLE terminal_sessions (
    id TEXT PRIMARY KEY,
    project_id TEXT REFERENCES projects(id),
    task_id TEXT REFERENCES tasks(id),
    host_instance_id TEXT NOT NULL,
    execution_environment_id TEXT NOT NULL,
    cwd TEXT NOT NULL,
    shell_executable TEXT NOT NULL,
    state TEXT NOT NULL,
    created_ms INTEGER NOT NULL,
    updated_ms INTEGER NOT NULL,
    last_event_seq INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_terminal_sessions_host ON terminal_sessions(host_instance_id);

CREATE TABLE agent_sessions (
    id TEXT PRIMARY KEY,
    task_id TEXT REFERENCES tasks(id),
    terminal_session_id TEXT REFERENCES terminal_sessions(id),
    provider TEXT NOT NULL,
    adapter_version TEXT NOT NULL,
    external_conversation_id TEXT,
    state TEXT NOT NULL,
    state_confidence TEXT NOT NULL,
    capabilities_json TEXT NOT NULL,
    last_event_seq INTEGER NOT NULL DEFAULT 0,
    created_ms INTEGER NOT NULL,
    updated_ms INTEGER NOT NULL
);

CREATE INDEX idx_agents_task ON agent_sessions(task_id, updated_ms DESC);
CREATE TABLE workspace_layouts (
    id TEXT PRIMARY KEY,
    structure_json TEXT NOT NULL,
    host_instance_id TEXT,
    process_start_token TEXT,
    updated_ms INTEGER NOT NULL
);
