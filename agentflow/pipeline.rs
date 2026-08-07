use evalflow::{Metric, TestCase};
use graph_flow::{Context, Task, TaskResult, error::Result};
use guardflow::Guard;

/// The `context` key [`AgentOpsTask`] reads an expected/reference output from, for
/// metrics like [`evalflow::metrics::F1Score`] or [`evalflow::metrics::ExactMatch`]
/// that need one. Set it with `context.set(EXPECTED_OUTPUT_KEY, "...")` before
/// running the task; metrics that don't need an expected output ignore it.
pub const EXPECTED_OUTPUT_KEY: &str = "agentops::expected_output";

/// Wraps a [`Task`] with the guard-then-evaluate half of the AgentOps lifecycle.
///
/// If a [`Guard`] is set, the task is retried (up to `max_attempts`) until its
/// response passes; the last attempt is used if none do. Every [`Metric`] then
/// scores the (possibly retried) response — against [`EXPECTED_OUTPUT_KEY`] in
/// `context`, if the caller set one — and writes the score into `context` as
/// `eval::<metric name>`, so anything downstream — another task, your own
/// logging — can read it without `evalflow` needing to know anything about
/// `graph-flow`'s execution model.
pub struct AgentOpsTask<T> {
    inner: T,
    guard: Option<Guard>,
    max_attempts: usize,
    metrics: Vec<Box<dyn Metric>>,
}

impl<T> AgentOpsTask<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            guard: None,
            max_attempts: 3,
            metrics: Vec::new(),
        }
    }

    pub fn with_guard(mut self, guard: Guard) -> Self {
        self.guard = Some(guard);
        self
    }

    pub fn with_max_attempts(mut self, max_attempts: usize) -> Self {
        self.max_attempts = max_attempts.max(1);
        self
    }

    pub fn with_metric(mut self, metric: impl Metric + 'static) -> Self {
        self.metrics.push(Box::new(metric));
        self
    }
}

#[async_trait::async_trait]
impl<T: Task> Task for AgentOpsTask<T> {
    fn id(&self) -> &str {
        self.inner.id()
    }

    async fn run(&self, context: Context) -> Result<TaskResult> {
        let result = match &self.guard {
            Some(guard) => {
                let mut last = None;
                let mut chosen = None;
                for _ in 0..self.max_attempts {
                    let attempt = self.inner.run(context.clone()).await?;
                    match &attempt.response {
                        Some(text) if !guard.check(text).passed => last = Some(attempt),
                        _ => {
                            chosen = Some(attempt);
                            break;
                        }
                    }
                }
                chosen.or(last).expect("max_attempts is at least 1")
            }
            None => self.inner.run(context.clone()).await?,
        };

        if let Some(text) = &result.response {
            let mut case = TestCase::new(String::new(), text.clone());
            if let Some(expected) = context.get::<String>(EXPECTED_OUTPUT_KEY) {
                case = case.expected(expected);
            }
            for metric in &self.metrics {
                let score = metric.score(&case);
                let _ = context.set(format!("eval::{}", metric.name()), score);
            }
        }

        Ok(result)
    }
}
