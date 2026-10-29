//! Where a mind runs. [`Worker`] keeps calls off the frame thread; [`Inline`] answers
//! at once, which keeps tests and the journey bot deterministic.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::Instant;

use crate::mind::{Decision, DecisionRequest, LineDraft, LineRequest, Mind, MindError, Reply};

/// One request for a mind.
#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    Decide(DecisionRequest),
    Line(LineRequest),
}

#[derive(Clone, Debug, PartialEq)]
pub enum JobResult {
    Decide(Result<Reply<Decision>, MindError>),
    Line(Result<Reply<LineDraft>, MindError>),
}

/// A finished job.
#[derive(Clone, Debug, PartialEq)]
pub struct Done {
    pub ticket: u64,
    pub result: JobResult,
    /// Wall time the call took. Diagnostic only; never enters the world.
    pub latency_ms: u64,
}

fn run(mind: &mut dyn Mind, ticket: u64, job: &Job) -> Done {
    let start = Instant::now();
    let result = match job {
        Job::Decide(r) => JobResult::Decide(mind.decide(r)),
        Job::Line(r) => JobResult::Line(mind.line(r)),
    };
    Done {
        ticket,
        result,
        latency_ms: start.elapsed().as_millis() as u64,
    }
}

/// Runs jobs for the host. `submit` and `poll` never block.
pub trait Executor {
    /// Queues a job. `false` means it's full: the host takes the default instead.
    fn submit(&mut self, ticket: u64, job: Job) -> bool;
    /// Jobs finished since the last poll.
    fn poll(&mut self) -> Vec<Done>;
    /// Jobs queued or running.
    fn in_flight(&self) -> usize;
}

/// Answers each job inside `submit`. For [`crate::FakeMind`] and
/// [`crate::RecordedMind`]: results are ready on the next poll, every time.
pub struct Inline {
    mind: Box<dyn Mind>,
    done: Vec<Done>,
}

impl Inline {
    pub fn new(mind: impl Mind + 'static) -> Self {
        Self {
            mind: Box::new(mind),
            done: Vec::new(),
        }
    }
}

impl Executor for Inline {
    fn submit(&mut self, ticket: u64, job: Job) -> bool {
        let mut done = run(&mut *self.mind, ticket, &job);
        // Inline answers are part of a deterministic run: no wall time in them.
        done.latency_ms = 0;
        self.done.push(done);
        true
    }

    fn poll(&mut self) -> Vec<Done> {
        std::mem::take(&mut self.done)
    }

    fn in_flight(&self) -> usize {
        self.done.len()
    }
}

/// Runs a mind on its own thread, one job at a time, with a bounded queue.
pub struct Worker {
    tx: Sender<(u64, Job)>,
    rx: Receiver<Done>,
    in_flight: usize,
    capacity: usize,
}

impl Worker {
    /// Starts the thread. At most `capacity` jobs are queued or running.
    pub fn spawn(mind: impl Mind + 'static, capacity: usize) -> Self {
        let (tx, jobs) = mpsc::channel::<(u64, Job)>();
        let (done_tx, rx) = mpsc::channel();
        let mut mind = mind;
        std::thread::Builder::new()
            .name("galaxy-mind".into())
            .spawn(move || {
                // Ends when the host drops the sender. Never joined: a call in flight
                // at shutdown must not hold up the game.
                while let Ok((ticket, job)) = jobs.recv() {
                    if done_tx.send(run(&mut mind, ticket, &job)).is_err() {
                        break;
                    }
                }
            })
            .expect("spawning the mind worker thread");
        Self {
            tx,
            rx,
            in_flight: 0,
            capacity: capacity.max(1),
        }
    }
}

impl Executor for Worker {
    fn submit(&mut self, ticket: u64, job: Job) -> bool {
        if self.in_flight >= self.capacity || self.tx.send((ticket, job)).is_err() {
            return false;
        }
        self.in_flight += 1;
        true
    }

    fn poll(&mut self) -> Vec<Done> {
        let mut out = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(done) => {
                    self.in_flight = self.in_flight.saturating_sub(1);
                    out.push(done);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.in_flight = 0;
                    break;
                }
            }
        }
        out
    }

    fn in_flight(&self) -> usize {
        self.in_flight
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeMind;
    use crate::mind::Question;
    use std::time::Duration;

    fn job() -> Job {
        Job::Decide(DecisionRequest {
            key: "sk.test".into(),
            instructions: String::new(),
            state: serde_json::Value::Null,
            question: Question::choice(&[("a".into(), "a".into()), ("b".into(), "b".into())]),
        })
    }

    #[test]
    fn worker_answers_off_thread_and_respects_capacity() {
        let mut w = Worker::spawn(FakeMind::new(7), 2);
        assert!(w.submit(1, job()));
        assert!(w.submit(2, job()));
        assert!(!w.submit(3, job()), "queue is full");
        let mut got = Vec::new();
        for _ in 0..200 {
            got.extend(w.poll());
            if got.len() == 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(got.iter().map(|d| d.ticket).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(w.in_flight(), 0);
    }
}
