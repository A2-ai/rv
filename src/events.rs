use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

type Handler = Arc<dyn Fn(&serde_json::Value) + Send + Sync>;
static HANDLER: Mutex<Option<Handler>> = Mutex::new(None);

/// Register the process-wide handler. Panics if called twice.
pub fn on<F>(handler: F)
where
    F: Fn(&serde_json::Value) + Send + Sync + 'static,
{
    let mut guard = HANDLER.lock().unwrap();
    assert!(guard.is_none());
    *guard = Some(Arc::new(handler));
}

/// Emit an event. No-op if no handler is installed.
pub fn emit<E: Serialize>(event: &E) {
    let handler = HANDLER.lock().unwrap().clone();
    let Some(handler) = handler else {
        return;
    };
    let value = serde_json::to_value(event).expect("event failed to serialize to JSON");
    handler(&value);
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Task {
    pub id: String,
    pub label: String,
    pub parent: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskResult {
    Ok,
    Failed,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    TaskStarted {
        #[serde(flatten)]
        task: Task,
    },
    TaskFinished {
        #[serde(flatten)]
        task: Task,
        result: TaskResult,
        time_ms: u64,
    },
}

/// Emit `TaskStarted`, run `f`, then emit `TaskFinished` with the result and elapsed time.
pub fn with_task<T, E>(task: Task, f: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    emit(&Event::TaskStarted { task: task.clone() });
    let start = std::time::Instant::now();
    let out = f();
    emit(&Event::TaskFinished {
        task,
        result: if out.is_ok() {
            TaskResult::Ok
        } else {
            TaskResult::Failed
        },
        time_ms: start.elapsed().as_millis() as u64,
    });
    out
}
