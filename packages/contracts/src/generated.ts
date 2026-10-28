// Generated from Rust. Run pnpm contracts:generate to update.

export const PROTOCOL_VERSION = 1 as const;
export const MAX_CONTROL_BYTES = 1048576 as const;

export type Id = string;

export type ProtocolVersion = 1;

export type ControlMessageType = "request" | "response" | "event";

export type ControlEnvelope<T> = {
  /**
   * Supported protocol version, serialized as the number 1.
   */
  v: ProtocolVersion;
  /**
   * Request/response correlation identifier, or an event identifier.
   */
  id: Id;
  /**
   * Wire key `type`, not a Rust or TypeScript discriminant for authorization.
   */
  type: ControlMessageType;
  /**
   * Scoped operation name. The host must validate and authorize dispatch.
   */
  method: string;
  /**
   * Method-specific typed data, never an unrestricted command string.
   */
  data: T;
};

export type ErrorCode =
  | "UNAUTHORIZED"
  | "FORBIDDEN"
  | "NOT_FOUND"
  | "CONFLICT"
  | "UNSUPPORTED_CAPABILITY"
  | "STALE_REVISION"
  | "HOST_OFFLINE"
  | "RATE_LIMITED"
  | "PROVIDER_ERROR"
  | "INTERNAL"
  | "UNSUPPORTED_VERSION"
  | "INVALID_REQUEST"
  | "PAYLOAD_TOO_LARGE";

export type ApiError = {
  /**
   * Stable classification for programmatic handling.
   */
  code: ErrorCode;
  /**
   * Human-readable, non-sensitive diagnostic.
   */
  message: string;
};

export type HostStatusResponse = {
  /**
   * Host package version.
   */
  host_version: string;
  /**
   * Local control protocol version.
   */
  protocol_version: number;
  /**
   * Current process instance identifier.
   */
  instance_id: string;
  /**
   * Operating-system process ID.
   */
  pid: number;
  /**
   * OS process start identity for PID-reuse detection.
   */
  process_start: string;
  /**
   * Start time in Unix epoch milliseconds.
   */
  started_ms: number;
  /**
   * Whether startup completed and the host is accepting clients.
   */
  ready: boolean;
};

export type HostStatusEvent =
  | {
      kind: "snapshot";
      /**
       * Current host status.
       */
      status: HostStatusResponse;
    }
  | {
      kind: "status";
      /**
       * Current host status.
       */
      status: HostStatusResponse;
    }
  | {
      kind: "reconnecting";
      /**
       * Consecutive failed attempt count.
       */
      attempt: number;
    }
  | {
      kind: "offline";
      /**
       * Stable typed failure.
       */
      error: ApiError;
    };

export type Project = {
  /**
   * Stable project identifier.
   */
  id: Id;
  /**
   * Display name.
   */
  name: string;
  /**
   * Canonical project root.
   */
  root_path: string;
  /**
   * Execution environment that owns the path.
   */
  execution_environment: ExecutionEnvironment;
  /**
   * Optional repository identity.
   */
  repo_id: Id | null;
  /**
   * Workspace trust state.
   */
  trust: ProjectTrust;
  /**
   * Creation time in Unix epoch milliseconds.
   */
  created_ms: bigint;
  /**
   * Last open time in Unix epoch milliseconds.
   */
  last_opened_ms: bigint;
};

export type ProjectTrust = "untrusted" | "trusted";

export type ProjectListRequest = Record<symbol, never>;

export type ProjectListResponse = {
  /**
   * Projects ordered by last open time.
   */
  projects: Array<Project>;
};

export type ProjectRegisterRequest = {
  /**
   * Absolute directory path selected by the user.
   */
  path: string;
  /**
   * Optional display name; the directory name is used when omitted.
   */
  name: string | null;
};

export type ProjectRemoveRequest = {
  /**
   * Project identifier.
   */
  id: Id;
};

export type ProjectRemoveResponse = {
  /**
   * Whether a project row was removed.
   */
  removed: boolean;
};

export type TerminalSpawnRequest = {
  /**
   * Working directory; omitted means the host process directory.
   */
  cwd: string | null;
  /**
   * Optional explicit shell executable.
   */
  shell: string | null;
  /**
   * Stable per-pane history identity; the host maps it to a private HISTFILE.
   */
  history_id: Id | null;
  /**
   * Initial rows.
   */
  rows: number;
  /**
   * Initial columns.
   */
  cols: number;
};

export type TerminalSpawnResponse = {
  /**
   * Stream/session identifier.
   */
  stream_id: Id;
  /**
   * Resolved shell executable.
   */
  shell: string;
  /**
   * Canonical working directory.
   */
  cwd: string;
  /**
   * Initial rows.
   */
  rows: number;
  /**
   * Initial columns.
   */
  cols: number;
};

export type TerminalRenderConfig = {
  /**
   * Maximum scrollback lines retained by the renderer.
   */
  scrollback_lines: number;
};

export type TerminalSnapshotRequest = {
  /**
   * Target stream/session.
   */
  stream_id: Id;
};

export type TerminalSnapshotResponse = {
  /**
   * Recent raw PTY bytes, oldest first.
   */
  data: Array<number>;
};

export type TerminalInputRequest = {
  /**
   * Target stream/session.
   */
  stream_id: Id;
  /**
   * Terminal input text.
   */
  data: string;
};

export type TerminalResizeRequest = {
  /**
   * Target stream/session.
   */
  stream_id: Id;
  /**
   * Rows.
   */
  rows: number;
  /**
   * Columns.
   */
  cols: number;
};

export type TerminalCloseRequest = {
  /**
   * Target stream/session.
   */
  stream_id: Id;
};

export type TerminalLayoutNode =
  | {
      kind: "leaf";
      /**
       * Stable pane identifier.
       */
      id: Id;
      /**
       * Display title fallback.
       */
      title: string;
      /**
       * Host stream/session identifier when one was created.
       */
      session_id: Id | null;
      /**
       * Stable history identity used for the private shell history file.
       */
      history_id: Id;
      /**
       * Last resolved working directory.
       */
      cwd: string | null;
      /**
       * Last resolved shell executable.
       */
      shell: string | null;
      /**
       * Lines scrolled up from the bottom when the layout was saved.
       */
      scroll_offset: number;
      /**
       * Whether the saved session is known to be lost.
       */
      session_lost: boolean;
    }
  | {
      kind: "split";
      /**
       * Stable split identifier.
       */
      id: Id;
      /**
       * Split orientation.
       */
      direction: TerminalSplitDirection;
      /**
       * First-child ratio in the range 0.15–0.85.
       */
      ratio: number;
      /**
       * First subtree.
       */
      first: TerminalLayoutNode;
      /**
       * Second subtree.
       */
      second: TerminalLayoutNode;
    };

export type TerminalSplitDirection = "horizontal" | "vertical";

export type TerminalTabLayout = {
  /**
   * Stable tab identifier.
   */
  id: Id;
  /**
   * User-visible tab title.
   */
  title: string;
  /**
   * Pane tree.
   */
  root: TerminalLayoutNode;
  /**
   * Active pane inside this tab.
   */
  active_pane_id: Id;
  /**
   * Maximized pane, when present.
   */
  zoomed_pane_id: Id | null;
};

export type WorkspaceLayout = {
  /**
   * Layout schema version.
   */
  version: number;
  /**
   * Tabs in display order.
   */
  tabs: Array<TerminalTabLayout>;
  /**
   * Active tab, when any.
   */
  active_tab_id: Id | null;
};

export type WorkspaceLayoutGetRequest = Record<symbol, never>;

export type WorkspaceLayoutSaveRequest = {
  /**
   * Layout to persist.
   */
  layout: WorkspaceLayout;
};

export type WorkspaceLayoutSnapshot = {
  /**
   * Saved layout, or none when absent/corrupt.
   */
  layout: WorkspaceLayout | null;
  /**
   * Current host instance identifier.
   */
  host_instance_id: string;
  /**
   * Current host process start identity.
   */
  process_start: string;
  /**
   * Host instance that saved the layout, when present.
   */
  layout_host_instance_id: string | null;
  /**
   * Process start identity that saved the layout, when present.
   */
  layout_process_start: string | null;
};

export type ExecutionEnvironment =
  | { kind: "native-mac" }
  | { kind: "native-linux" }
  | { kind: "native-windows" }
  | {
      kind: "wsl";
      /**
       * Distribution name as configured on the Windows host.
       */
      distro: string;
    };

export type TaskState =
  | "draft"
  | "ready"
  | "running"
  | "blocked"
  | "review"
  | "approved"
  | "integrated"
  | "failed"
  | "canceled";

export type AgentStatus =
  | "starting"
  | "working"
  | "idle"
  | "needs_input"
  | "needs_approval"
  | "turn_finished"
  | "failed"
  | "exited"
  | "unknown";
