//! Print deterministic declarations from the authoritative Rust wire types.

use muxyard_contracts::{
    AgentStatus, ApiError, ControlEnvelope, ControlMessageType, ErrorCode, ExecutionEnvironment,
    HostStatusEvent, HostStatusResponse, Id, MAX_CONTROL_BYTES, PROTOCOL_VERSION, Project,
    ProjectListRequest, ProjectListResponse, ProjectRegisterRequest, ProjectRemoveRequest,
    ProjectRemoveResponse, ProjectTrust, ProtocolVersion, TaskState, TerminalCloseRequest,
    TerminalInputRequest, TerminalLayoutNode, TerminalRenderConfig, TerminalResizeRequest,
    TerminalSnapshotRequest, TerminalSnapshotResponse, TerminalSpawnRequest, TerminalSpawnResponse,
    TerminalSplitDirection, TerminalTabLayout, WorkspaceLayout, WorkspaceLayoutGetRequest,
    WorkspaceLayoutSaveRequest, WorkspaceLayoutSnapshot,
};
use ts_rs::{Config, TS};

fn main() {
    let config = Config::new();
    println!("// Generated from Rust. Run pnpm contracts:generate to update.\n");
    println!("export const PROTOCOL_VERSION = {PROTOCOL_VERSION} as const;");
    println!("export const MAX_CONTROL_BYTES = {MAX_CONTROL_BYTES} as const;\n");

    for declaration in [
        Id::decl(&config),
        ProtocolVersion::decl(&config),
        ControlMessageType::decl(&config),
        ControlEnvelope::<()>::decl(&config),
        ErrorCode::decl(&config),
        ApiError::decl(&config),
        HostStatusResponse::decl(&config),
        HostStatusEvent::decl(&config),
        Project::decl(&config),
        ProjectTrust::decl(&config),
        ProjectListRequest::decl(&config),
        ProjectListResponse::decl(&config),
        ProjectRegisterRequest::decl(&config),
        ProjectRemoveRequest::decl(&config),
        ProjectRemoveResponse::decl(&config),
        TerminalSpawnRequest::decl(&config),
        TerminalSpawnResponse::decl(&config),
        TerminalRenderConfig::decl(&config),
        TerminalSnapshotRequest::decl(&config),
        TerminalSnapshotResponse::decl(&config),
        TerminalInputRequest::decl(&config),
        TerminalResizeRequest::decl(&config),
        TerminalCloseRequest::decl(&config),
        TerminalLayoutNode::decl(&config),
        TerminalSplitDirection::decl(&config),
        TerminalTabLayout::decl(&config),
        WorkspaceLayout::decl(&config),
        WorkspaceLayoutGetRequest::decl(&config),
        WorkspaceLayoutSaveRequest::decl(&config),
        WorkspaceLayoutSnapshot::decl(&config),
        ExecutionEnvironment::decl(&config),
        TaskState::decl(&config),
        AgentStatus::decl(&config),
    ] {
        println!("export {declaration}\n");
    }
}
