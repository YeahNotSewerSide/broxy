# Logging with Tracing

This project uses the `tracing` crate for structured logging with spans and events. The logging system is configured to output pretty-formatted logs to stdout with rich metadata.

## Features

- **Pretty Console Output**: Colorized, formatted logs with timestamps
- **Structured Logging**: Support for spans, events, and metadata
- **Environment-based Configuration**: Control log levels via `RUST_LOG` environment variable
- **Rich Metadata**: Thread IDs, file names, line numbers, and timestamps
- **Span Events**: Automatic span lifecycle tracking

## Usage

### Basic Usage

```bash
# Run with default log level (INFO)
cargo run

# Run with DEBUG level
RUST_LOG=debug cargo run

# Run with TRACE level (most verbose)
RUST_LOG=trace cargo run
```

### Log Levels

- `error`: Only error messages
- `warn`: Warning and error messages
- `info`: Info, warning, and error messages (default)
- `debug`: Debug, info, warning, and error messages
- `trace`: All messages (most verbose)

### Module-specific Logging

```bash
# Set different levels for different modules
RUST_LOG=broxy=debug,hyper=info,tokio=warn cargo run

# Only show errors from dependencies, debug from our code
RUST_LOG=error,broxy=debug cargo run
```

## Log Format

The logs include:
- **Timestamp**: RFC 3339 format
- **Log Level**: Color-coded (ERROR=red, WARN=yellow, INFO=green, DEBUG=blue, TRACE=cyan)
- **Thread Information**: Thread ID and name
- **File Location**: File name and line number
- **Message**: The actual log message
- **Spans**: Parent span context for structured logging

## Example Output

```
2024-01-15T10:30:45.123Z  INFO broxy{thread="main"}: Starting Broxy proxy server
2024-01-15T10:30:45.124Z DEBUG broxy{thread="main"}: Configured upstream: Upstream { address: 0.0.0.0:3006, root_path: /, use_ssl: false }
2024-01-15T10:30:45.125Z DEBUG broxy{thread="main"}: Created service 1 for POST /login
2024-01-15T10:30:45.126Z DEBUG broxy{thread="main"}: Created service 2 for GET /api/user with middleware
2024-01-15T10:30:45.127Z  INFO broxy{thread="main"}: Created service bundle with 2 services
2024-01-15T10:30:45.128Z  INFO broxy{thread="main"}: Starting server on 0.0.0.0:8181
2024-01-15T10:30:45.129Z  INFO broxy{thread="main"}: Server started successfully, accepting connections
```

## Adding Logging to Your Code

### Basic Logging

```rust
use tracing::{info, debug, warn, error};

info!("This is an info message");
debug!("This is a debug message");
warn!("This is a warning message");
error!("This is an error message");
```

### Structured Logging with Spans

```rust
use tracing::{info_span, instrument};

#[instrument(skip(complex_data))]
async fn process_request(complex_data: Vec<u8>) {
    let _span = info_span!("process_request", data_size = complex_data.len());
    let _enter = _span.enter();
    
    info!("Processing request");
    // ... your code here
}
```

### Conditional Logging

```rust
use tracing::debug;

if cfg!(debug_assertions) {
    debug!("This only appears in debug builds");
}
```

## Configuration

The logging system is initialized in `src/logging.rs` with the following features:

- **Pretty formatting**: Human-readable output
- **ANSI colors**: Color-coded log levels
- **Thread information**: Thread IDs and names
- **File locations**: Source file and line numbers
- **Timestamps**: RFC 3339 format
- **Span events**: Automatic span lifecycle tracking

You can modify the logging configuration by editing the `init_logging_*` functions in `src/logging.rs`. 