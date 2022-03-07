use std::sync::Arc;
use std::collections::VecDeque;

use log::debug;
use log::info;
use chrono::Utc;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::RwLock;
use tokio::sync::Semaphore;
use tokio::sync::mpsc;

mod error;
mod data;

pub use crate::error::Result;
pub use crate::error::Error;
pub use crate::data::Job;
pub use crate::data::Action;
pub use crate::data::Capacity;
pub use async_trait::async_trait;

use crate::data::QueuedJob;
use crate::data::ProtectedQueuedJob;
use crate::data::ProtectedQueuedJobs;


pub async fn loop_forever<T: Job>(
    capacity: Capacity,
    receiver: mpsc::Receiver<T>,
    initial_jobs: Vec<T>,
) {
    let jobs: VecDeque<_> = initial_jobs.into_iter()
        .map(wrap_job)
        .collect();
    let jobs = Arc::new(RwLock::new(jobs));

    // The maximum number of running jobs
    let slots = Arc::new(Semaphore::new(capacity.max_running_jobs as usize));

    let shutdown = Arc::new(RwLock::new(false));

    tokio::spawn(receive(shutdown.clone(), receiver, jobs.clone()));

    loop {
        {
            let shutdown = shutdown.read().await;
            if *shutdown == true {
                return ();
            }
        }
        let sleep_time = sweep_once(capacity.clone(), jobs.clone(), slots.clone()).await;
        {
            let shutdown = shutdown.read().await;
            if *shutdown == true {
                return ();
            }
        }

        // TODO: select shutdown
        tokio::time::sleep(std::time::Duration::from_secs(sleep_time as u64)).await;
    }
}

async fn receive<T: Job>(shutdown: Arc<RwLock<bool>>, receiver: mpsc::Receiver<T>, jobs: ProtectedQueuedJobs<T>) {
    let mut receiver = receiver;
    loop {
        match receiver.recv().await {
            None => {
                let mut shutdown = shutdown.write().await;
                *shutdown = true;
                return ();
            },
            Some(job) => {
                let job = wrap_job(job);
                let mut jobs = jobs.write().await;
                jobs.push_back(job);
            }
        }
    }
}

fn wrap_job<T: Job>(job: T) -> ProtectedQueuedJob<T> {
    let queued = QueuedJob {
        job,
        ticket: Arc::new(Semaphore::new(1)),
    };
    Arc::new(RwLock::new(queued))
}

async fn sweep_once<T: Job>(
    capacity: Capacity,
    jobs: ProtectedQueuedJobs<T>,
    slots: Arc<Semaphore>,
) -> u32 {
    // Locks the job queue during the sweep
    let mut jobs = jobs.write().await;

    let total_jobs = jobs.len();
    let mut processed_jobs = 0;

    let mut sleep_time = capacity.sweep_sleep_seconds_default;

    // Loop over the jobs to dispatch
    loop {
        // Every job in the queue is accounted for, run out of jobs
        if processed_jobs >= total_jobs {
            debug!("All {} jobs in the queue are accounted for, terminating this pass", total_jobs);
            break;
        }

        match slots.clone().try_acquire_owned() {
            // Run out of slots
            Err(_) => {
                debug!("No available slots, terminating this pass");
                break;
            }

            Ok(slot) => match jobs.pop_front() {
                // Run out of jobs
                None => {
                    debug!("Job queue is empty, terminating this pass");
                    break;
                }

                Some(job) => {
                    processed_jobs += 1;

                    let job_display = job.read().await.job.show();

                    let (put_back, seconds_left) = schedule_one(slot, job.clone()).await;
                    if let Some(seconds_left) = seconds_left {
                        sleep_time = sleep_time.min(seconds_left as u32);
                        sleep_time = capacity.sweep_sleep_seconds_min.max(sleep_time);
                        sleep_time = capacity.sweep_sleep_seconds_max.min(sleep_time);
                    }

                    if put_back {
                        jobs.push_back(job);
                    } else {
                        info!("Job {} is discarded", &job_display);
                    }
                }
            }
        }
    }

    sleep_time
}


async fn schedule_one<T: Job>(
    slot: OwnedSemaphorePermit,
    job: ProtectedQueuedJob<T>,
) -> (bool, Option<u32>) {
    let job_clone = job.clone();
    let job = job.read().await;

    let action = job.job.get_action();

    let no_schedule_hint = None;

    match action {
        Action::DoNothing => (true, no_schedule_hint),
        Action::Terminate => (false, no_schedule_hint),
        Action::RunAfter {delay} => {
            if delay > 0 {
                (true, Some(delay))
            } else {
                match job.ticket.clone().try_acquire_owned() {
                    Err(_) => {
                        debug!("Skip dispatching job {}, for there is one already running", &job.job.show());
                        (true, no_schedule_hint)
                    }
                    Ok(ticket) => {
                        tokio::spawn(run_job(slot, ticket, job_clone));
                        (true, no_schedule_hint)
                    }
                }
            }
        }
    }
}

async fn run_job<T: Job>(slot: OwnedSemaphorePermit, ticket: OwnedSemaphorePermit, job: ProtectedQueuedJob<T>) {

    // Run the job without holding the write lock
    let outcome = {
        let job = job.read().await;
        job.job.run().await
    };
    {
        let mut job = job.write().await;
        let now = Utc::now().timestamp();
        job.job.update(now, outcome).await;
    }

    std::mem::drop(slot);
    std::mem::drop(ticket);
}
