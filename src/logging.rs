use tracing::Level;
use tracing_subscriber::{
    fmt::{format::FmtSpan, time::LocalTime},
    FmtSubscriber,
};

/// Initialize the logging system with pretty console output
pub fn init_logging() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::DEBUG)
        .with_target(false)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .with_span_events(FmtSpan::CLOSE)
        .with_timer(LocalTime::rfc_3339())
        .with_ansi(true)
        .pretty()
        .with_level(true)
        .with_target(false)
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;
    
    tracing::info!("Logging system initialized");
    Ok(())
}

/// Initialize logging with a specific log level
pub fn init_logging_with_level(level: Level) -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_target(false)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .with_span_events(FmtSpan::CLOSE)
        .with_timer(LocalTime::rfc_3339())
        .with_ansi(true)
        .pretty()
        .with_level(true)
        .with_target(false)
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;
    
    tracing::info!("Logging system initialized with level: {}", level);
    Ok(())
}

/// Initialize logging from environment variable RUST_LOG
pub fn init_logging_from_env() -> Result<(), Box<dyn std::error::Error>> {
    let env_filter = tracing_subscriber::EnvFilter::from_default_env();
    
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(env_filter)
        .with_target(false)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .with_span_events(FmtSpan::CLOSE)
        .with_timer(LocalTime::rfc_3339())
        .with_ansi(true)
        .pretty()
        .with_level(true)
        .with_target(false)
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;
    
    tracing::info!("Logging system initialized from environment");
    Ok(())
} 