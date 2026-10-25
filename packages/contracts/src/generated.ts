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
