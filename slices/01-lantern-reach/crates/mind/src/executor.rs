//! Where a mind runs. [`Worker`] keeps calls off the frame thread; [`Inline`] answers
//! at once, which keeps tests and the journey bot deterministic.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::time::{Duration, Instant};

use crate::calllog::CallId;
use crate::mind::{
    Decision, DecisionRequest, LineDraft, LineRequest, LiveDraft, LiveRequest, Mind, MindError,
    Reply,
};

/// One request for a mind.
#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    Decide(DecisionRequest),
    Line(LineRequest),
    /// A live-layer flourish (chat completions through the auto router).
    Live(LiveRequest),
}

#[derive(Clone, Debug, PartialEq)]
pub enum JobResult {
    Decide(Reply<Decision>),
    Line(Reply<LineDraft>),
    Live(Reply<LiveDraft>),
}

impl JobResult {
    /// The call log row the call was written to, if any.
    pub fn call(&self) -> Option<&CallId> {
        match self {
            JobResult::Decide(r) => r.call.as_ref(),
            JobResult::Line(r) => r.call.as_ref(),
            JobResult::Live(r) => r.call.as_ref(),
        }
    }

    /// Attempts, cost and model, whatever the outcome.
    pub fn usage(&self) -> (u8, f64, &str, u64) {
        match self {
            JobResult::Decide(r) => (r.attempts, r.cost_usd, &r.model, r.hold_ticks),
            JobResult::Line(r) => (r.attempts, r.cost_usd, &r.model, r.hold_ticks),
            JobResult::Live(r) => (r.attempts, r.cost_usd, &r.model, r.hold_ticks),
        }
    }
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
        Job::Live(r) => JobResult::Live(mind.live(r)),
    };
    Done {
        ticket,
        result,
        latency_ms: start.elapsed().as_millis() as u64,
    }
}

/// A job given up before it started: nothing was sent, nothing is owed.
fn cancelled(ticket: u64, job: &Job) -> Done {
    Done {
        ticket,
        result: match job {
            Job::Decide(_) => JobResult::Decide(Reply::none(MindError::Cancelled)),
            Job::Line(_) => JobResult::Line(Reply::none(MindError::Cancelled)),
            Job::Live(_) => JobResult::Live(Reply::none(MindError::Cancelled)),
        },
        latency_ms: 0,
    }
}

/// Runs jobs for the host. `submit` and `poll` never block; `close` does, once, at
/// shutdown.
pub trait Executor {
    /// Queues a job. `false` means it's full: the host takes the default instead.
    fn submit(&mut self, ticket: u64, job: Job) -> bool;
    /// Jobs finished since the last poll.
    fn poll(&mut self) -> Vec<Done>;
    /// Jobs queued or running.
    fn in_flight(&self) -> usize;
    /// Takes no more jobs, cancels those not started yet (they come back as
    /// [`MindError::Cancelled`] with nothing sent) and waits up to `wait` for the one
    /// running, so its billed response can be booked. Returns every job finished.
    fn close(&mut self, _wait: Duration) -> Vec<Done> {
        self.poll()
    }
}

/// Answers each job inside `submit`. For [`crate::FakeMind`] and
/// [`crate::RecordedMind`]: results are ready on the next poll, every time. A job the
/// mind doesn't admit (a replayed `busy`) is refused like a full queue.
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
        if !self.mind.admits(&job) {
            return false;
        }
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
///
/// The thread is never joined on drop: a call in flight must not hold up the game. A
/// response that finishes after its host is gone can't be booked, so call
/// [`crate::MindHost::shutdown`] (which calls [`Executor::close`]) before the final
/// save: it cancels queued jobs unsent and waits, bounded, for the running one.
pub struct Worker {
    tx: Option<Sender<(u64, Job)>>,
    rx: Receiver<Done>,
    in_flight: usize,
    capacity: usize,
    closing: Arc<AtomicBool>,
}

impl Worker {
    /// Starts the thread. At most `capacity` jobs are queued or running.
    pub fn spawn(mind: impl Mind + 'static, capacity: usize) -> Self {
        let (tx, jobs) = mpsc::channel::<(u64, Job)>();
        let (done_tx, rx) = mpsc::channel();
        let closing = Arc::new(AtomicBool::new(false));
        let stop = closing.clone();
        let mut mind = mind;
        std::thread::Builder::new()
            .name("galaxy-mind".into())
            .spawn(move || {
                // Ends when the host drops the sender. Once closing, jobs still queued
                // come back cancelled without a call.
                while let Ok((ticket, job)) = jobs.recv() {
                    let done = if stop.load(Ordering::SeqCst) {
                        cancelled(ticket, &job)
                    } else {
                        run(&mut mind, ticket, &job)
                    };
                    if done_tx.send(done).is_err() {
                        break;
                    }
                }
            })
            .expect("spawning the mind worker thread");
        Self {
            tx: Some(tx),
            rx,
            in_flight: 0,
            capacity: capacity.max(1),
            closing,
        }
    }
}

impl Executor for Worker {
    fn submit(&mut self, ticket: u64, job: Job) -> bool {
        let Some(tx) = &self.tx else {
            return false;
        };
        if self.in_flight >= self.capacity || tx.send((ticket, job)).is_err() {
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

    fn close(&mut self, wait: Duration) -> Vec<Done> {
        self.closing.store(true, Ordering::SeqCst);
        // The thread ends once it has answered what is queued.
        self.tx = None;
        let deadline = Instant::now() + wait;
        let mut out = self.poll();
        while self.in_flight > 0 {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(done) => {
                    self.in_flight -= 1;
                    out.push(done);
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    self.in_flight = 0;
                    break;
                }
            }
        }
        out
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
            max_attempts: 2,
            deadline_ms: 10_000,
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
