/** Display the foundation's empty workspace without implying a live host. */
export function App() {
  return (
    <div className="workspace">
      <header className="workspace-header">
        <span className="wordmark">
          <span className="yard-mark" aria-hidden="true">
            <i />
            <i />
            <i />
          </span>
          Muxyard
        </span>
        <span className="build-label">Development scaffold</span>
      </header>
      <main className="empty-workspace">
        <div className="empty-content">
          <p className="eyebrow">Your workspace</p>
          <h1>No project open</h1>
          <p className="empty-description">
            A clear space for your next build.
          </p>
          <p className="scope-note">
            This is the application foundation. Terminals, agents, and remote
            access are not connected yet.
          </p>
        </div>
      </main>
      <footer className="workspace-footer">
        <span>Local application</span>
        <span>Host not connected</span>
      </footer>
    </div>
  );
}
