//! OpenTelemetry integration for distributed tracing
//!
//! Provides observability across the agent fleet:
//! - Local agents (tmux panes)
//! - Remote agents (Sprites/Firecracker VMs)
//! - Cross-iteration correlation for long-running loops
//!
//! This is NOT usage tracking - it's distributed tracing for YOUR fleet,
//! so you can see what 50 Sprites are doing across a week-long refactor.

use color_eyre::eyre::{Result, WrapErr};
use opentelemetry::global;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::runtime::Tokio;
use opentelemetry_sdk::trace::Tracer;
use tracing::Subscriber;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::registry::LookupSpan;

/// Configuration for OTEL telemetry.
/// Reserved for Phase 1: Full OTEL integration with Jaeger/Honeycomb.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct TelemetryConfig {
    /// OTLP endpoint (e.g., "http://localhost:4317")
    pub endpoint: String,
    /// Service name for traces
    pub service_name: String,
    /// Service version
    pub service_version: String,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:4317".to_string(),
            service_name: "rehoboam".to_string(),
            service_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

#[allow(dead_code)]
impl TelemetryConfig {
    /// Create a new config with the given endpoint.
    pub fn with_endpoint(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            ..Default::default()
        }
    }
}

/// Initialize OTEL tracer provider.
///
/// Returns a tracer that can be used with tracing-opentelemetry.
/// Reserved for Phase 1: Full OTEL integration.
#[allow(dead_code)]
fn init_tracer(config: &TelemetryConfig) -> Result<Tracer> {
    use opentelemetry::KeyValue;
    use opentelemetry_sdk::trace::Config;
    use opentelemetry_sdk::Resource;

    let resource = Resource::new(vec![
        KeyValue::new("service.name", config.service_name.clone()),
        KeyValue::new("service.version", config.service_version.clone()),
    ]);

    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint(&config.endpoint),
        )
        .with_trace_config(Config::default().with_resource(resource))
        .install_batch(Tokio)
        .wrap_err_with(|| format!("Failed to initialize OTLP exporter at {}", config.endpoint))?;

    Ok(tracer)
}

/// Create an OpenTelemetry layer for tracing-subscriber.
///
/// This bridges tracing spans to OTEL traces.
/// Reserved for Phase 1: Full OTEL integration.
#[allow(dead_code)]
pub fn otel_layer<S>(config: &TelemetryConfig) -> Result<OpenTelemetryLayer<S, Tracer>>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    let tracer = init_tracer(config)?;
    Ok(tracing_opentelemetry::layer().with_tracer(tracer))
}

/// Shutdown OTEL tracer provider gracefully.
///
/// Should be called before application exit to flush pending traces.
/// Reserved for Phase 1: Full OTEL integration.
#[allow(dead_code)]
pub fn shutdown() {
    global::shutdown_tracer_provider();
    tracing::debug!("OTEL tracer provider shut down");
}

/// Check if an OTEL endpoint is reachable.
///
/// Returns true if the endpoint responds, false otherwise.
/// This is a non-blocking check that times out after 1 second.
/// Reserved for Phase 1: Full OTEL integration.
#[allow(dead_code)]
pub async fn check_endpoint(endpoint: &str) -> bool {
    use std::time::Duration;
    use tokio::time::timeout;

    // Parse the endpoint to get host and port
    let url = match url::Url::parse(endpoint) {
        Ok(u) => u,
        Err(_) => return false,
    };

    let host = match url.host_str() {
        Some(h) => h,
        None => return false,
    };

    let port = url.port().unwrap_or(4317);
    let addr = format!("{}:{}", host, port);

    // Try to connect with timeout
    match timeout(
        Duration::from_secs(1),
        tokio::net::TcpStream::connect(&addr),
    )
    .await
    {
        Ok(Ok(_)) => {
            tracing::debug!("OTEL endpoint {} is reachable", endpoint);
            true
        }
        Ok(Err(e)) => {
            tracing::debug!("OTEL endpoint {} not reachable: {}", endpoint, e);
            false
        }
        Err(_) => {
            tracing::debug!("OTEL endpoint {} connection timed out", endpoint);
            false
        }
    }
}

/// Get trace context for propagation to remote agents.
///
/// Returns W3C Trace Context headers (traceparent, tracestate).
/// Used when spawning Sprites to correlate their traces with the parent.
/// Reserved for Phase 1: Cross-agent trace propagation.
#[allow(dead_code)]
pub fn extract_trace_context() -> Option<String> {
    use opentelemetry::trace::TraceContextExt;
    use opentelemetry::Context;

    let context = Context::current();
    let span = context.span();
    let span_context = span.span_context();

    if span_context.is_valid() {
        // Format as W3C traceparent
        let traceparent = format!(
            "00-{}-{}-{:02x}",
            span_context.trace_id(),
            span_context.span_id(),
            span_context.trace_flags().to_u8()
        );
        Some(traceparent)
    } else {
        None
    }
}

/// OTEL metrics for agent fleet monitoring
///
/// These metrics provide fleet-level visibility:
/// - Active spans
/// - Tool call latencies
/// - Iteration durations
pub mod metrics {
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Counter for total tool calls across all agents
    static TOOL_CALLS: AtomicU64 = AtomicU64::new(0);

    /// Counter for total iterations across all loop agents
    static ITERATIONS: AtomicU64 = AtomicU64::new(0);

    /// Record a tool call
    pub fn record_tool_call() {
        TOOL_CALLS.fetch_add(1, Ordering::Relaxed);
        tracing::trace!(monotonic_counter.tool_calls = 1, "tool call recorded");
    }

    /// Record a loop iteration
    pub fn record_iteration() {
        ITERATIONS.fetch_add(1, Ordering::Relaxed);
        tracing::trace!(monotonic_counter.iterations = 1, "iteration recorded");
    }

    /// Record tool latency histogram
    pub fn record_tool_latency(tool: &str, latency_ms: u64) {
        tracing::info!(
            histogram.tool_latency_ms = latency_ms,
            tool = tool,
            "tool latency"
        );
    }

    /// Record iteration duration histogram.
    /// Reserved for Phase 1: Per-iteration timing metrics.
    #[allow(dead_code)]
    pub fn record_iteration_duration(iteration: u32, duration_secs: u64) {
        tracing::info!(
            histogram.iteration_duration_secs = duration_secs,
            iteration = iteration,
            "iteration duration"
        );
    }

    /// Get current tool call count
    pub fn get_tool_calls() -> u64 {
        TOOL_CALLS.load(Ordering::Relaxed)
    }

    /// Get current iteration count
    pub fn get_iterations() -> u64 {
        ITERATIONS.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_config_default() {
        let config = TelemetryConfig::default();
        assert_eq!(config.endpoint, "http://localhost:4317");
        assert_eq!(config.service_name, "rehoboam");
    }

    #[test]
    fn test_telemetry_config_with_endpoint() {
        let config = TelemetryConfig::with_endpoint("http://jaeger:4317");
        assert_eq!(config.endpoint, "http://jaeger:4317");
    }

    #[test]
    fn test_metrics_counters() {
        let initial_tool_calls = metrics::get_tool_calls();
        metrics::record_tool_call();
        assert_eq!(metrics::get_tool_calls(), initial_tool_calls + 1);

        let initial_iterations = metrics::get_iterations();
        metrics::record_iteration();
        assert_eq!(metrics::get_iterations(), initial_iterations + 1);
    }
}
