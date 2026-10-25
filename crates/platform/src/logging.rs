//! Structured logging with explicit redaction for sensitive values.

use std::{fmt, path::Path};

use tracing::Subscriber;
use tracing_subscriber::fmt::MakeWriter;

use crate::{LogFormat, LoggingConfig, PlatformError, paths::open_private_append};

/// A value that must never appear in logs.
#[derive(Clone, PartialEq, Eq)]
pub struct Sensitive<T>(T);

impl<T> Sensitive<T> {
    /// Wrap a sensitive value.
    pub fn new(value: T) -> Self {
        Self(value)
    }

    /// Access the value at the point where it is actually needed.
    pub fn expose(&self) -> &T {
        &self.0
    }

    /// Consume the wrapper and return the value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> fmt::Debug for Sensitive<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[redacted]")
    }
}

impl<T> fmt::Display for Sensitive<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[redacted]")
    }
}

/// Logging initialization failures.
#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    /// The log file could not be opened securely.
    #[error(transparent)]
    Platform(#[from] PlatformError),
    /// A global subscriber was already installed in this process.
    #[error("global tracing subscriber is already initialized")]
    AlreadyInitialized,
}

/// Build a subscriber for a specific writer. Tests use this with scoped dispatch.
pub fn build_subscriber<W>(config: &LoggingConfig, writer: W) -> Box<dyn Subscriber + Send + Sync>
where
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    match config.format {
        LogFormat::Json => Box::new(
            tracing_subscriber::fmt::Subscriber::builder()
                .json()
                .with_ansi(false)
                .with_target(false)
                .with_max_level(config.level.as_filter())
                .with_writer(writer)
                .finish(),
        ),
        LogFormat::Text => Box::new(
            tracing_subscriber::fmt::Subscriber::builder()
                .compact()
                .with_ansi(false)
                .with_target(false)
                .with_max_level(config.level.as_filter())
                .with_writer(writer)
                .finish(),
        ),
    }
}

/// Install the process-wide file logger. Call once during host startup.
pub fn init_file_logging(config: &LoggingConfig, log_path: &Path) -> Result<(), LoggingError> {
    let file = open_private_append(log_path)?;
    let subscriber = build_subscriber(config, file);
    tracing::subscriber::set_global_default(subscriber)
        .map_err(|_| LoggingError::AlreadyInitialized)?;
    tracing::info!(
        path = %log_path.display(),
        "structured logging initialized"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{self, Write},
        sync::{Arc, Mutex},
    };

    use super::*;
    use crate::{LogFormat, LogLevel};

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Buffer {
        type Writer = Buffer;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    fn config(format: LogFormat) -> LoggingConfig {
        LoggingConfig {
            level: LogLevel::Info,
            format,
        }
    }

    #[test]
    fn sensitive_values_are_redacted_in_text_logs() {
        let buffer = Buffer::default();
        let subscriber = build_subscriber(&config(LogFormat::Text), buffer.clone());
        tracing::subscriber::with_default(subscriber, || {
            let secret = Sensitive::new("never-log-this-value");
            tracing::info!(secret = %secret, "test event");
        });
        let output = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        assert!(!output.contains("never-log-this-value"));
        assert!(output.contains("[redacted]"));
        assert!(output.contains("test event"));
    }

    #[test]
    fn json_logs_are_structured_and_redacted() {
        let buffer = Buffer::default();
        let subscriber = build_subscriber(&config(LogFormat::Json), buffer.clone());
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(token = %Sensitive::new("secret-token"), "json event");
        });
        let output = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        let line = output.lines().next().unwrap();
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(value["level"], "INFO");
        assert_eq!(value["fields"]["message"], "json event");
        assert_eq!(value["fields"]["token"], "[redacted]");
        assert!(!output.contains("secret-token"));
    }

    #[test]
    fn sensitive_debug_and_display_do_not_expose_content() {
        let secret = Sensitive::new("hidden");
        assert_eq!(format!("{secret:?}"), "[redacted]");
        assert_eq!(format!("{secret}"), "[redacted]");
        assert_eq!(secret.expose(), &"hidden");
        assert_eq!(Sensitive::new("hidden").into_inner(), "hidden");
    }

    #[test]
    fn file_logging_does_not_echo_secret_values() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("muxyard.log");
        let file = open_private_append(&path).unwrap();
        let subscriber = build_subscriber(&config(LogFormat::Text), Mutex::new(file));
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(secret = %Sensitive::new("file-secret"), "file event");
        });
        let output = fs::read_to_string(&path).unwrap();
        assert!(!output.contains("file-secret"));
        assert!(output.contains("[redacted]"));
    }
}
