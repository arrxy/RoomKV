use tracing_subscriber::fmt::format::FmtSpan;

pub struct LoggerGuard {
    _guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

pub fn init_logger() -> LoggerGuard {
    #[cfg(feature = "local-logs")]
    {
        // In local mode, our macros use println!, so no tracing setup needed.
        LoggerGuard { _guard: None }
    }

    #[cfg(not(feature = "local-logs"))]
    {
        let file_appender = tracing_appender::rolling::daily("logs", "roomkv.log");

        let (non_blocking_writer, guard) = tracing_appender::non_blocking(file_appender);

        tracing_subscriber::fmt()
            .with_writer(non_blocking_writer)
            .with_ansi(false)
            .with_target(true)
            .with_thread_ids(true)
            .with_span_events(FmtSpan::NONE)
            .init();

        LoggerGuard {
            _guard: Some(guard),
        }
    }
}

#[macro_export]
macro_rules! rk_info {
    ($($arg:tt)*) => {{
        #[cfg(feature = "local-logs")]
        {
            println!("[INFO] {}", format!($($arg)*));
        }

        #[cfg(not(feature = "local-logs"))]
        {
            tracing::info!($($arg)*);
        }
    }};
}

#[macro_export]
macro_rules! rk_debug {
    ($($arg:tt)*) => {{
        #[cfg(feature = "local-logs")]
        {
            println!("[DEBUG] {}", format!($($arg)*));
        }

        #[cfg(not(feature = "local-logs"))]
        {
            tracing::debug!($($arg)*);
        }
    }};
}

#[macro_export]
macro_rules! rk_warn {
    ($($arg:tt)*) => {{
        #[cfg(feature = "local-logs")]
        {
            println!("[WARN] {}", format!($($arg)*));
        }

        #[cfg(not(feature = "local-logs"))]
        {
            tracing::warn!($($arg)*);
        }
    }};
}

#[macro_export]
macro_rules! rk_error {
    ($($arg:tt)*) => {{
        #[cfg(feature = "local-logs")]
        {
            eprintln!("[ERROR] {}", format!($($arg)*));
        }

        #[cfg(not(feature = "local-logs"))]
        {
            tracing::error!($($arg)*);
        }
    }};
}
