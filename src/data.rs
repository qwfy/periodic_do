use std::sync::Arc;
use std::collections::VecDeque;

use tokio::sync::Semaphore;
use tokio::sync::RwLock;
use async_trait::async_trait;


#[async_trait]
pub trait Job: std::marker::Sync + std::marker::Send + 'static {
    type Outcome: std::marker::Sync + std::marker::Send;

    /// Show the job for logging purpose
    fn show(&self) -> String;

    /// Given the current status of the job, what to to next?
    fn get_action(&self) -> Action;

    /// Run the job.
    ///
    /// Note that, if running the job is not guaranteed to be success,
    /// and the failure should be handled,
    /// then you can use a `Result<_, _>` as the `Outcome`,
    /// and later handle it in the `update` function.
    async fn run(&self) -> Self::Outcome;

    /// Update the job with the outcome of the run.
    ///
    /// `run` and `update ` is called in succession,
    /// the `update` should advance the state of the job - if your `Outcome` indicates so.
    /// You can choose to not modify `self` in this function call - if you do not want to advance the state of the job.
    ///
    /// # Arguments:
    ///
    /// * `time_run` - the unix time when the [Self::run()] is finished
    async fn update(&mut self, time_run: i64, outcome: Self::Outcome) -> ();
}

/// Action to be taken given the current status of the job
pub enum Action {
    /// Do not schedule the job to be run
    DoNothing,

    /// Schedule the job to be run after the specified seconds.
    /// A delay of `0` means to run the job ASAP
    RunAfter {delay: u32},

    /// Terminate this job, after this, the job will not be considered anymore
    Terminate,
}


pub(crate) struct QueuedJob<T: Job> {
    pub(crate) job: T,
    pub(crate) ticket: Arc<Semaphore>,
}

pub(crate) type ProtectedQueuedJob<T> = Arc<RwLock<QueuedJob<T>>>;

pub(crate) type ProtectedQueuedJobs<T> = Arc<RwLock<VecDeque<ProtectedQueuedJob<T>>>>;


#[derive(Clone)]
pub struct Capacity {
    pub max_running_jobs: u32,

    pub sweep_sleep_seconds_default: u32,
    pub sweep_sleep_seconds_min: u32,
    pub sweep_sleep_seconds_max: u32,
}


impl std::default::Default for Capacity {
    fn default() -> Self {
        Capacity {
            max_running_jobs: 10,
            sweep_sleep_seconds_default: 5,
            sweep_sleep_seconds_min: 1,
            sweep_sleep_seconds_max: 60,
        }
    }
}