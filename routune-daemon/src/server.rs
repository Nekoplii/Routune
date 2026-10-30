use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};
use tonic::{Request, Response, Status};
use tracing::debug;

use crate::audio::TelemetrySample;

pub mod proto {
    tonic::include_proto!("routune");
}

pub trait TelemetryMetricsProvider: Send + Sync {
    fn get_sink_metrics(&self) -> Vec<proto::SinkMetric>;
    fn get_topology(&self) -> Option<proto::GraphTopology>;
    fn get_master_metrics(&self) -> Option<(f32, f32)>;
    fn is_active(&self) -> bool;
}

#[cfg(target_os = "linux")]
impl TelemetryMetricsProvider for crate::pipewire_bridge::PipeWireBridgeService {
    fn get_sink_metrics(&self) -> Vec<proto::SinkMetric> {
        self.get_sink_metrics()
    }

    fn get_topology(&self) -> Option<proto::GraphTopology> {
        Some(self.get_topology())
    }

    fn get_master_metrics(&self) -> Option<(f32, f32)> {
        Some(self.get_master_metrics())
    }

    fn is_active(&self) -> bool {
        self.is_active.load(Ordering::Relaxed)
    }
}

pub struct ControlServiceImpl {
    is_audio_active: Arc<AtomicBool>,
    telemetry_tx: broadcast::Sender<proto::AudioTelemetry>,
    provider: Option<Arc<dyn TelemetryMetricsProvider>>,
}

impl ControlServiceImpl {
    pub fn new(
        is_audio_active: Arc<AtomicBool>,
        telemetry_tx: broadcast::Sender<proto::AudioTelemetry>,
        provider: Option<Arc<dyn TelemetryMetricsProvider>>,
    ) -> Self {
        Self {
            is_audio_active,
            telemetry_tx,
            provider,
        }
    }
}

#[tonic::async_trait]
impl proto::routune_control_server::RoutuneControl for ControlServiceImpl {
    type StreamTelemetryStream =
        Pin<Box<dyn Stream<Item = Result<proto::AudioTelemetry, Status>> + Send + 'static>>;

    async fn ping(
        &self,
        request: Request<proto::PingRequest>,
    ) -> Result<Response<proto::PingResponse>, Status> {
        let req = request.into_inner();
        debug!("Received ping from client: {:?}", req.client_id);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        let audio_active = self.is_audio_active.load(Ordering::Relaxed)
            || self.provider.as_ref().map(|p| p.is_active()).unwrap_or(false);

        let response = proto::PingResponse {
            message: "pong".to_string(),
            timestamp_ms: now,
            audio_active,
        };

        Ok(Response::new(response))
    }

    async fn stream_telemetry(
        &self,
        request: Request<proto::TelemetryRequest>,
    ) -> Result<Response<Self::StreamTelemetryStream>, Status> {
        let req = request.into_inner();
        debug!("Client requested telemetry stream with target_fps={}", req.target_fps);

        let rx = self.telemetry_tx.subscribe();
        let stream = BroadcastStream::new(rx).filter_map(|res| match res {
            Ok(telemetry) => Some(Ok(telemetry)),
            Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(_)) => None,
        });

        Ok(Response::new(Box::pin(stream)))
    }
}

pub async fn run_telemetry_dispatcher(
    mut consumer: rtrb::Consumer<TelemetrySample>,
    telemetry_tx: broadcast::Sender<proto::AudioTelemetry>,
    provider: Option<Arc<dyn TelemetryMetricsProvider>>,
) {
    let mut interval = tokio::time::interval(Duration::from_millis(16));
    let mut seq = 0u64;

    loop {
        interval.tick().await;

        let mut latest_peak = -100.0f32;
        let mut sum_rms_db = 0.0f32;
        let mut sample_count = 0usize;

        while let Ok(sample) = consumer.pop() {
            if sample.peak_volume_db > latest_peak {
                latest_peak = sample.peak_volume_db;
            }
            sum_rms_db += sample.rms_volume_db;
            sample_count += 1;
        }

        let mut rms_db = if sample_count > 0 {
            sum_rms_db / sample_count as f32
        } else {
            -100.0
        };

        let (sink_metrics, topology) = if let Some(p) = &provider {
            if let Some((m_peak, m_rms)) = p.get_master_metrics() {
                if m_peak > latest_peak {
                    latest_peak = m_peak;
                }
                if m_rms > -99.0 {
                    rms_db = if sample_count > 0 {
                        (rms_db + m_rms) * 0.5
                    } else {
                        m_rms
                    };
                }
            }
            (p.get_sink_metrics(), p.get_topology())
        } else {
            (Vec::new(), None)
        };

        let telemetry = proto::AudioTelemetry {
            peak_volume_db: latest_peak,
            rms_volume_db: rms_db,
            spectrum: Vec::new(),
            sequence_number: seq,
            sink_metrics,
            topology,
        };
        seq = seq.wrapping_add(1);

        // Discard error if no active subscribers
        let _ = telemetry_tx.send(telemetry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::routune_control_server::RoutuneControl;

    #[tokio::test]
    async fn test_ping_response() {
        let is_active = Arc::new(AtomicBool::new(true));
        let (tx, _) = broadcast::channel(16);
        let service = ControlServiceImpl::new(is_active, tx, None);

        let req = tonic::Request::new(proto::PingRequest {
            client_id: "test-client".to_string(),
        });
        let res = service.ping(req).await.expect("ping failed").into_inner();

        assert_eq!(res.message, "pong");
        assert!(res.audio_active);
        assert!(res.timestamp_ms > 0);
    }

    #[tokio::test]
    async fn test_stream_telemetry_flow() {
        let is_active = Arc::new(AtomicBool::new(true));
        let (tx, _) = broadcast::channel(16);
        let service = ControlServiceImpl::new(is_active, tx.clone(), None);

        let req = tonic::Request::new(proto::TelemetryRequest { target_fps: 60 });
        let res = service
            .stream_telemetry(req)
            .await
            .expect("stream_telemetry failed")
            .into_inner();

        let mut stream = res;

        // Emit sample from background
        let telemetry_frame = proto::AudioTelemetry {
            peak_volume_db: -3.5,
            rms_volume_db: -9.2,
            spectrum: vec![0.1, 0.5],
            sequence_number: 42,
            sink_metrics: vec![proto::SinkMetric {
                sink_id: "sink-1".to_string(),
                sink_name: "Sink 1".to_string(),
                peak_volume_db: -5.0,
                rms_volume_db: -12.0,
            }],
            topology: Some(proto::GraphTopology {
                nodes: vec![proto::GraphNodeInfo {
                    id: "n1".into(),
                    name: "N1".into(),
                    node_type: "TestNode".into(),
                    bypassed: false,
                    peak_volume_db: -5.0,
                    rms_volume_db: -12.0,
                }],
                edges: vec![],
            }),
        };
        tx.send(telemetry_frame).expect("send broadcast failed");

        let received = stream.next().await.expect("expected stream item").expect("stream item error");
        assert_eq!(received.sequence_number, 42);
        assert!((received.peak_volume_db - (-3.5)).abs() < 0.001);
        assert_eq!(received.sink_metrics.len(), 1);
        assert_eq!(received.sink_metrics[0].sink_id, "sink-1");
        assert!(received.topology.is_some());
    }

    #[tokio::test]
    async fn test_telemetry_dispatcher_raw_telemetry() {
        let (mut producer, consumer) = rtrb::RingBuffer::new(16);
        let (tx, mut rx) = broadcast::channel(16);

        let dispatcher_handle = tokio::spawn(run_telemetry_dispatcher(consumer, tx, None));

        // 1. Send active sample
        producer
            .push(TelemetrySample {
                peak_volume_db: -6.0,
                rms_volume_db: -12.0,
            })
            .expect("push failed");

        // Wait for first telemetry frame with active audio: raw instantaneously sent
        let first = rx.recv().await.expect("receive failed");
        assert!((first.peak_volume_db - (-6.0)).abs() < 0.01);
        assert!((first.rms_volume_db - (-12.0)).abs() < 0.01);

        // 2. Next tick with no samples: immediate raw silence (-100.0 dB) without smoothing or decay hold
        let second = rx.recv().await.expect("receive failed");
        assert_eq!(second.peak_volume_db, -100.0);
        assert_eq!(second.rms_volume_db, -100.0);

        dispatcher_handle.abort();
    }
}