# Periodic Do

This is a library for running periodic tasks:

- Implement the `Job` trait for your domain specific data type `T`, which represents the task to be run
- Send instances of `T` via a `tokio::sync::mpsc::channel` to `periodic_do::loop_forever`
- This library will periodically poll `T` for two things:
  - Is this `T` up to be run? If so, run it and update the job state
  - Has this `T` reached a terminal state? In which case it will be forgotten

## Project status
Usable, but has rough edges, evaluate before using it in production.

## Example Usage

```rust
use tokio;

#[tokio::main]
async fn main() -> Result<(), Error> {
  let (sender, receiver) = tokio::sync::mpsc::channel(1000);
  
  let unfinished_jobs = load_unfinished_jobs_from_database().await?;
  
  // Start the scheduler
  let capacity = periodic_do::Capacity {
      max_running_jobs: 10,
      sweep_sleep_seconds_default: 5,
      sweep_sleep_seconds_min: 1,
      sweep_sleep_seconds_max: 60,
  };
  tokio::spawn(async move {
      periodic_do::loop_forever(
          capacity, receiver, unfinished_jobs
      ).await
  });
  
  loop {
      // T implements `Job` trait
      let some_task: T = get_a_task_from_some_where().await;
      sender.send(some_task).await
  }
  
  Ok(())
}
```