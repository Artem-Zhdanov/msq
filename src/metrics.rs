use opentelemetry::metrics::Counter;
use opentelemetry::metrics::Histogram;
use opentelemetry::metrics::ObservableGauge;
use opentelemetry_sdk::metrics::PeriodicReader;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::runtime::Tokio;
use std::sync::Arc;
use std::time::Duration;

pub struct Metrics {
    pub latency: Histogram<u64>,
    pub send_time: Histogram<u64>,
    pub errors: Counter<u64>,

    // pub recv: Gauge<u64>,
    // pub sent: Gauge<u64>,
    // pub lost: Gauge<u64>,
    // pub retrans: Gauge<u64>,
    // pub sent_bytes: Gauge<u64>,
    // pub recv_bytes: Gauge<u64>,
    // pub lost_bytes: Gauge<u64>,

    // Tokio runtime metrics: https://docs.rs/tokio/latest/tokio/runtime/struct.RuntimeMetrics.html
    _tokio_num_alive_tasks: ObservableGauge<u64>,
    _tokio_global_queue_depth: ObservableGauge<u64>,
    //  _tokio_spawned_tasks_count: ObservableCounter<u64>,
}

pub fn init_metrics() -> Arc<Metrics> {
    // Don't forget to set and export `OTEL_EXPORTER_OTLP_METRICS_ENDPOINT` envar.
    unsafe {
        std::env::set_var("OTEL_EXPORTER_OTLP_METRICS_PROTOCOL", "grpc");
        std::env::set_var("OTEL_EXPORTER_OTLP_INSECURE", "true");
        std::env::set_var("OTEL_SERVICE_NAME", "transport-layer-test");
    }
    let resource = opentelemetry_sdk::Resource::default();

    let metric_exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_tonic()
        .build()
        .expect("Failed to build OTLP metrics exporter");

    let meter_provider = SdkMeterProvider::builder()
        .with_reader(
            PeriodicReader::builder(metric_exporter, Tokio)
                .with_interval(Duration::from_secs(30))
                .with_timeout(Duration::from_secs(5))
                .build(),
        )
        .with_resource(resource)
        .build();

    opentelemetry::global::set_meter_provider(meter_provider.clone());
    let meter = &opentelemetry::global::meter("quic");
    let boundaries = vec![
        0.0, 5.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0, 110.0, 120.0, 130.0,
        140.0, 150.0, 160.0, 170.0, 180.0, 190.0, 200.0, 210.0, 220.0, 230.0, 240.0, 250.0, 280.0,
        300.0, 1000.0, 5000.0, 10000.0,
    ];
    let metrics = Arc::new(Metrics {
        latency: meter
            .u64_histogram("transport_layer_latency")
            .with_boundaries(boundaries.clone())
            .build(),
        send_time: meter
            .u64_histogram("transport_layer_send_time")
            .with_boundaries(boundaries)
            .build(),
        errors: meter.u64_counter("transport_layer_error").build(),
        // sent: meter.u64_gauge("quic_sent").build(),
        // recv: meter.u64_gauge("quic_recv").build(),
        // lost: meter.u64_gauge("quic_lost").build(),
        // retrans: meter.u64_gauge("quic_retrans").build(),
        // sent_bytes: meter.u64_gauge("quic_sent_bytes").build(),
        // recv_bytes: meter.u64_gauge("quic_recv_bytes").build(),
        // lost_bytes: meter.u64_gauge("quic_lost_bytes").build(),
        _tokio_num_alive_tasks: meter
            .u64_observable_gauge("node_tokio_num_alive_tasks")
            .with_callback(move |observer| {
                observer.observe(
                    tokio::runtime::Handle::current()
                        .metrics()
                        .num_alive_tasks() as u64,
                    &[],
                )
            })
            .build(),

        _tokio_global_queue_depth: meter
            .u64_observable_gauge("node_tokio_global_queue_depth")
            .with_callback(move |observer| {
                observer.observe(
                    tokio::runtime::Handle::current()
                        .metrics()
                        .global_queue_depth() as u64,
                    &[],
                )
            })
            .build(),
        // _tokio_spawned_tasks_count: meter
        //     .u64_observable_counter("node_tokio_spawned_tasks_count")
        //     .with_callback(move |observer| {
        //         observer.observe(
        //             tokio::runtime::Handle::current()
        //                 .metrics()
        //                 .spawned_tasks_count(),
        //             &[],
        //         );
        //     })
        //     .build(),
    });
    metrics
}
