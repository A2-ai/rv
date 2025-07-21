use anyhow::Result;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

fn debug_print(msg: &str) {
    if std::env::var("RV_TEST_DEBUG").is_ok() {
        println!("🐛 DEBUG: {}", msg);
    }
}

#[derive(Debug, Clone)]
pub enum StepStatus {
    Pending,
    Running,
    Completed,
}

pub struct StepCoordinator {
    step_status: Arc<Mutex<Vec<Vec<StepStatus>>>>, // [step_index][thread_index]
    thread_names: Vec<String>,
    step_waiters: Arc<(Mutex<Vec<bool>>, Condvar)>, // One bool per step for coordination
}

impl StepCoordinator {
    pub fn new(thread_names: Vec<String>, num_steps: usize) -> Self {
        let num_threads = thread_names.len();

        // Initialize step status - all steps start as Pending for all threads
        let step_status = Arc::new(Mutex::new(
            (0..num_steps)
                .map(|_| vec![StepStatus::Pending; num_threads])
                .collect(),
        ));

        let step_waiters = Arc::new((Mutex::new(vec![false; num_steps]), Condvar::new()));

        Self {
            step_status,
            thread_names,
            step_waiters,
        }
    }

    fn get_thread_index(&self, thread_name: &str) -> Option<usize> {
        self.thread_names
            .iter()
            .position(|name| name == thread_name)
    }

    pub fn wait_for_step_start(
        &self,
        step_index: usize,
        thread_name: &str,
        timeout: Option<Duration>,
    ) -> Result<()> {
        let thread_index = self
            .get_thread_index(thread_name)
            .ok_or_else(|| anyhow::anyhow!("Unknown thread: {}", thread_name))?;

        debug_print(&format!(
            "Thread {} waiting for step {} to start",
            thread_name, step_index
        ));

        // Mark this thread as ready for this step
        {
            let mut status = self.step_status.lock().unwrap();
            status[step_index][thread_index] = StepStatus::Running;
        }

        // Check if all threads are ready for this step
        let all_ready = {
            let status = self.step_status.lock().unwrap();
            status[step_index]
                .iter()
                .all(|s| matches!(s, StepStatus::Running | StepStatus::Completed))
        };

        if all_ready {
            debug_print(&format!(
                "All threads ready for step {}, proceeding",
                step_index
            ));
            let (lock, cvar) = &*self.step_waiters;
            let mut step_ready = lock.lock().unwrap();
            step_ready[step_index] = true;
            cvar.notify_all();
            return Ok(());
        }

        // Wait for other threads to be ready
        let (lock, cvar) = &*self.step_waiters;
        let mut step_ready = lock.lock().unwrap();

        let wait_result = if let Some(timeout_duration) = timeout {
            let start_time = Instant::now();
            loop {
                if step_ready[step_index] {
                    break Ok(());
                }

                let elapsed = start_time.elapsed();
                if elapsed >= timeout_duration {
                    break Err(anyhow::anyhow!(
                        "Timeout waiting for step {} start",
                        step_index
                    ));
                }

                let remaining = timeout_duration - elapsed;
                let (new_lock, timeout_result) = cvar.wait_timeout(step_ready, remaining).unwrap();
                step_ready = new_lock;

                if timeout_result.timed_out() {
                    break Err(anyhow::anyhow!(
                        "Timeout waiting for step {} start",
                        step_index
                    ));
                }
            }
        } else {
            while !step_ready[step_index] {
                step_ready = cvar.wait(step_ready).unwrap();
            }
            Ok(())
        };

        wait_result
    }

    pub fn notify_step_completed(&self, step_index: usize, thread_name: &str) -> Result<()> {
        let thread_index = self
            .get_thread_index(thread_name)
            .ok_or_else(|| anyhow::anyhow!("Unknown thread: {}", thread_name))?;

        debug_print(&format!(
            "Thread {} completed step {}",
            thread_name, step_index
        ));

        {
            let mut status = self.step_status.lock().unwrap();
            status[step_index][thread_index] = StepStatus::Completed;
        }

        // Step completion recorded in step_status above

        Ok(())
    }
}
