//! Typed persisted terminal layout and recovery snapshot.

use serde::{Deserialize, Serialize};

use crate::Id;

/// Maximum serialized workspace layout accepted by the host.
pub const MAX_WORKSPACE_LAYOUT_BYTES: usize = 256 * 1024;

/// Split orientation persisted with a pane tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub enum TerminalSplitDirection {
    /// Side-by-side panes.
    Horizontal,
    /// Stacked panes.
    Vertical,
}

/// One persisted terminal pane tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub enum TerminalLayoutNode {
    /// A single terminal pane.
    Leaf {
        /// Stable pane identifier.
        id: Id,
        /// Display title fallback.
        title: String,
        /// Host stream/session identifier when one was created.
        session_id: Option<Id>,
        /// Stable history identity used for the private shell history file.
        history_id: Id,
        /// Last resolved working directory.
        cwd: Option<String>,
        /// Last resolved shell executable.
        shell: Option<String>,
        /// Lines scrolled up from the bottom when the layout was saved.
        scroll_offset: u32,
        /// Whether the saved session is known to be lost.
        session_lost: bool,
    },
    /// A split containing two subtrees.
    Split {
        /// Stable split identifier.
        id: Id,
        /// Split orientation.
        direction: TerminalSplitDirection,
        /// First-child ratio in the range 0.15–0.85.
        ratio: f64,
        /// First subtree.
        first: Box<TerminalLayoutNode>,
        /// Second subtree.
        second: Box<TerminalLayoutNode>,
    },
}

/// One persisted terminal tab.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TerminalTabLayout {
    /// Stable tab identifier.
    pub id: Id,
    /// User-visible tab title.
    pub title: String,
    /// Pane tree.
    pub root: TerminalLayoutNode,
    /// Active pane inside this tab.
    pub active_pane_id: Id,
    /// Maximized pane, when present.
    pub zoomed_pane_id: Option<Id>,
}

/// Persisted workspace layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct WorkspaceLayout {
    /// Layout schema version.
    pub version: u16,
    /// Tabs in display order.
    pub tabs: Vec<TerminalTabLayout>,
    /// Active tab, when any.
    pub active_tab_id: Option<Id>,
}

/// Request to read the persisted workspace layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct WorkspaceLayoutGetRequest {}

/// Layout plus host identity used to decide whether saved sessions may be live.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct WorkspaceLayoutSnapshot {
    /// Saved layout, or none when absent/corrupt.
    pub layout: Option<WorkspaceLayout>,
    /// Current host instance identifier.
    pub host_instance_id: String,
    /// Current host process start identity.
    pub process_start: String,
    /// Host instance that saved the layout, when present.
    pub layout_host_instance_id: Option<String>,
    /// Process start identity that saved the layout, when present.
    pub layout_process_start: Option<String>,
}

/// Save request for the workspace layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct WorkspaceLayoutSaveRequest {
    /// Layout to persist.
    pub layout: WorkspaceLayout,
}
