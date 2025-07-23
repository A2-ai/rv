use anyhow::Result;
use std::sync::{Arc, Barrier};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

fn debug_print(msg: &str) {
    if std::env::var("RV_TEST_DEBUG").is_ok() {
        println!("🐛 DEBUG: {}", msg);
    }
}

pub struct StepCoordinator {
    barrier: Arc<Barrier>,
    abort_flag: Arc<AtomicBool>,
}

impl StepCoordinator {
    pub fn new(thread_names: Vec<String>, _num_steps: usize) -> Self {
        let num_threads = thread_names.len();
        Self {
            barrier: Arc::new(Barrier::new(num_threads)),
            abort_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn wait_for_step_start(
        &self,
        step_index: usize,
        thread_name: &str,
        _timeout: Option<Duration>,
    ) -> Result<()> {
        // Check if we should abort before waiting
        if self.abort_flag.load(Ordering::Relaxed) {
            return Err(anyhow::anyhow!(
                "Thread {} aborting at step {} due to test failure",
                thread_name, step_index
            ));
        }

        debug_print(&format!(
            "Thread {} hitting entry barrier for step {}",
            thread_name, step_index
        ));
        
        // Only use barrier if abort hasn't been signaled
        if !self.abort_flag.load(Ordering::Relaxed) {
            self.barrier.wait();
        }
        
        // Check again after barrier in case abort was signaled during wait
        if self.abort_flag.load(Ordering::Relaxed) {
            return Err(anyhow::anyhow!(
                "Thread {} aborting at step {} due to test failure",
                thread_name, step_index
            ));
        }
        
        debug_print(&format!(
            "Thread {} proceeding with step {}",
            thread_name, step_index
        ));

        Ok(())
    }

    pub fn notify_step_completed(&self, step_index: usize, thread_name: &str) -> Result<()> {
        debug_print(&format!(
            "Thread {} hitting exit barrier for step {}",
            thread_name, step_index
        ));
        
        // Only use barrier if abort hasn't been signaled
        if !self.abort_flag.load(Ordering::Relaxed) {
            self.barrier.wait();
        }
        
        debug_print(&format!(
            "Thread {} proceeding past step {}",
            thread_name, step_index
        ));

        Ok(())
    }

    pub fn signal_abort(&self) {
        debug_print("Signaling abort to all threads");
        self.abort_flag.store(true, Ordering::Relaxed);
    }
}
