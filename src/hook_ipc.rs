use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(unix)]
use interprocess::local_socket::tokio::Stream as LocalSocketStream;
#[cfg(unix)]
use interprocess::local_socket::traits::tokio::Stream as _;
#[cfg(unix)]
use interprocess::local_socket::{GenericFilePath, GenericNamespaced, ToFsName, ToNsName};
#[cfg(windows)]
use interprocess::os::windows::named_pipe::{pipe_mode, tokio::SendPipeStream};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio::sync::Notify;

use crate::config::HookIpcConfig;
use crate::hooks::HookPoint;

const IPC_EVENT_VERSION: u32 = 1;
const DEFAULT_QUEUE_CAPACITY: usize = 256;
const RECONNECT_DELAY: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Serialize)]
pub struct HookIpcEvent {
    pub version: u32,
    pub seq: u64,
    pub timestamp: String,
    pub session_id: String,
    pub point: HookPoint,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    pub payload: Value,
}

impl HookIpcEvent {
    pub fn new(
        seq: u64,
        session_id: impl Into<String>,
        point: HookPoint,
        tool: Option<String>,
        payload: Value,
    ) -> Self {
        Self {
            version: IPC_EVENT_VERSION,
            seq,
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: session_id.into(),
            point,
            tool,
            payload,
        }
    }
}

#[derive(Default)]
struct QueueState {
    items: VecDeque<HookIpcEvent>,
}

#[derive(Default)]
struct PublisherState {
    queue: Mutex<QueueState>,
    notify: Notify,
    seq: AtomicU64,
}

#[derive(Clone, Default)]
pub struct HookIpcPublisherHandle {
    inner: Option<Arc<HookIpcPublisherInner>>,
}

struct HookIpcPublisherInner {
    endpoint: String,
    state: Arc<PublisherState>,
}

impl HookIpcPublisherHandle {
    pub fn new(config: &HookIpcConfig) -> Self {
        if !config.enabled {
            return Self { inner: None };
        }

        let state = Arc::new(PublisherState::default());
        let inner = Arc::new(HookIpcPublisherInner {
            endpoint: config.effective_endpoint(),
            state: Arc::clone(&state),
        });

        tokio::spawn(run_worker(Arc::clone(&inner)));

        Self { inner: Some(inner) }
    }

    pub fn disabled() -> Self {
        Self { inner: None }
    }

    pub fn publish(&self, session_id: &str, point: HookPoint, tool: Option<&str>, payload: Value) {
        let Some(inner) = &self.inner else {
            return;
        };

        let seq = inner.state.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let event = HookIpcEvent::new(seq, session_id, point, tool.map(ToOwned::to_owned), payload);

        let mut queue = inner.state.queue.lock().expect("hook ipc queue poisoned");
        if queue.items.len() >= DEFAULT_QUEUE_CAPACITY {
            let _ = queue.items.pop_front();
        }
        queue.items.push_back(event);
        drop(queue);
        inner.state.notify.notify_one();
    }

    #[cfg(test)]
    pub fn queued_len_for_test(&self) -> usize {
        self.inner
            .as_ref()
            .map(|inner| {
                inner
                    .state
                    .queue
                    .lock()
                    .expect("hook ipc queue poisoned")
                    .items
                    .len()
            })
            .unwrap_or(0)
    }

    #[cfg(test)]
    pub fn pop_front_for_test(&self) -> Option<HookIpcEvent> {
        self.inner.as_ref().and_then(|inner| {
            inner
                .state
                .queue
                .lock()
                .expect("hook ipc queue poisoned")
                .items
                .pop_front()
        })
    }
}

async fn run_worker(inner: Arc<HookIpcPublisherInner>) {
    loop {
        wait_for_events(&inner.state).await;

        let mut stream = match connect_stream(&inner.endpoint).await {
            Ok(stream) => stream,
            Err(error) => {
                log::debug!(
                    "hook_ipc: failed to connect to {}: {}",
                    inner.endpoint,
                    error
                );
                tokio::time::sleep(RECONNECT_DELAY).await;
                continue;
            }
        };

        loop {
            wait_for_events(&inner.state).await;
            let event = pop_next_event(&inner.state)
                .expect("hook IPC queue became empty after wait_for_events returned");

            match write_event(&mut stream, &event).await {
                Ok(()) => {}
                Err(error) => {
                    log::debug!("hook_ipc: write failed to {}: {}", inner.endpoint, error);
                    break;
                }
            }
        }
    }
}

async fn wait_for_events(state: &PublisherState) {
    loop {
        let notified = state.notify.notified();
        if !state
            .queue
            .lock()
            .expect("hook ipc queue poisoned")
            .items
            .is_empty()
        {
            return;
        }
        notified.await;
    }
}

fn pop_next_event(state: &PublisherState) -> Option<HookIpcEvent> {
    state
        .queue
        .lock()
        .expect("hook ipc queue poisoned")
        .items
        .pop_front()
}

#[cfg(windows)]
type HookIpcStream = SendPipeStream<pipe_mode::Bytes>;

#[cfg(unix)]
type HookIpcStream = LocalSocketStream;

async fn connect_stream(endpoint: &str) -> std::io::Result<HookIpcStream> {
    #[cfg(windows)]
    {
        SendPipeStream::<pipe_mode::Bytes>::connect_by_path(endpoint).await
    }

    #[cfg(unix)]
    {
        let name = endpoint_to_name(endpoint)?;
        LocalSocketStream::connect(name).await
    }
}

#[cfg(unix)]
fn endpoint_to_name(endpoint: &str) -> std::io::Result<interprocess::local_socket::Name<'static>> {
    endpoint
        .to_fs_name::<GenericFilePath>()
        .or_else(|_| endpoint.to_ns_name::<GenericNamespaced>())
        .map(|name| name.into_owned())
}

async fn write_event(stream: &mut HookIpcStream, event: &HookIpcEvent) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(event).map_err(|error| {
        std::io::Error::other(format!("failed to serialize hook IPC event: {error}"))
    })?;
    line.push(b'\n');
    stream.write_all(&line).await?;
    stream.flush().await
}

pub fn empty_payload() -> Value {
    json!({})
}

pub fn ipc_tool_intent_payload(name: &str) -> Value {
    json!({"tool": name})
}

pub fn ipc_pre_tool_payload(name: &str, args: &Value) -> Value {
    json!({"tool": name, "arguments": args})
}

pub fn ipc_on_error_payload(error: &str, tool: Option<&str>, args: Option<&Value>) -> Value {
    let mut payload = json!({"error": error});
    if let Some(tool) = tool {
        payload["tool"] = Value::String(tool.to_string());
    }
    if let Some(args) = args {
        payload["arguments"] = args.clone();
    }
    payload
}

pub fn ipc_status_update_payload(status: &str) -> Value {
    json!({"status": status})
}

pub fn ipc_external_change_payload(paths: &[std::path::PathBuf]) -> Value {
    json!({
        "paths": paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_payload_is_object() {
        assert_eq!(empty_payload(), json!({}));
    }

    #[test]
    fn tool_intent_payload_omits_placeholder_args() {
        let payload = ipc_tool_intent_payload("read_file");
        assert_eq!(payload, json!({"tool": "read_file"}));
        assert!(payload.get("arguments").is_none());
    }

    #[test]
    fn event_serialization_shape_uses_empty_payload() {
        let event = HookIpcEvent::new(7, "sess-1", HookPoint::OnDone, None, empty_payload());
        let value = serde_json::to_value(event).expect("serializes");
        assert_eq!(value["version"], 1);
        assert_eq!(value["seq"], 7);
        assert_eq!(value["session_id"], "sess-1");
        assert_eq!(value["point"], "on_done");
        assert_eq!(value["payload"], json!({}));
        assert!(value.get("tool").is_none());
    }

    #[test]
    fn disabled_publisher_is_noop() {
        let publisher = HookIpcPublisherHandle::disabled();
        publisher.publish("sess", HookPoint::OnDone, None, empty_payload());
        assert_eq!(publisher.queued_len_for_test(), 0);
    }

    #[test]
    fn full_queue_drops_oldest() {
        let publisher = HookIpcPublisherHandle {
            inner: Some(Arc::new(HookIpcPublisherInner {
                endpoint: "unused".to_string(),
                state: Arc::new(PublisherState::default()),
            })),
        };

        for idx in 0..(DEFAULT_QUEUE_CAPACITY + 5) {
            publisher.publish("sess", HookPoint::OnDone, None, json!({"idx": idx}));
        }

        assert_eq!(publisher.queued_len_for_test(), DEFAULT_QUEUE_CAPACITY);
        let first = publisher
            .pop_front_for_test()
            .expect("first queued event exists");
        assert_eq!(first.payload["idx"], 5);
    }

    #[tokio::test]
    async fn connect_failure_does_not_panic() {
        let result = connect_stream("definitely-invalid-endpoint").await;
        assert!(result.is_err());
    }

    #[test]
    fn status_update_payload_shape() {
        assert_eq!(
            ipc_status_update_payload("retrying"),
            json!({"status": "retrying"})
        );
    }
}
