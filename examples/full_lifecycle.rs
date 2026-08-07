//! The AgentOps lifecycle in one task: a flaky "LLM" that sometimes answers too
//! short to be useful, guarded (retried until long enough), then scored against
//! a reference answer — the score lands in the shared Context, not just stdout,
//! so a real pipeline could log it, gate a deploy on it, or feed it to the next
//! task.

use std::sync::atomic::{AtomicUsize, Ordering};

use agentflow::eval::metrics::F1Score;
use agentflow::guard::Guard;
use agentflow::guard::validators::MinLength;
use agentflow::pipeline::{AgentOpsTask, EXPECTED_OUTPUT_KEY};
use async_trait::async_trait;
use graph_flow::{Context, NextAction, Task, TaskResult, error::Result};

struct FlakyLlm {
    attempts: AtomicUsize,
}

#[async_trait]
impl Task for FlakyLlm {
    fn id(&self) -> &str {
        "flaky_llm"
    }

    async fn run(&self, _context: Context) -> Result<TaskResult> {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
        let response = if attempt < 2 {
            "not sure"
        } else {
            "the quick brown fox jumps over the lazy dog"
        };
        Ok(TaskResult::new(
            Some(response.to_string()),
            NextAction::Continue,
        ))
    }
}

#[tokio::main]
async fn main() {
    let task = AgentOpsTask::new(FlakyLlm {
        attempts: AtomicUsize::new(0),
    })
    .with_guard(Guard::new().with(MinLength(15)))
    .with_max_attempts(3)
    .with_metric(F1Score::default());

    let context = Context::new();
    context
        .set(
            EXPECTED_OUTPUT_KEY,
            "the quick brown fox jumps over a lazy dog",
        )
        .unwrap();

    let result = task.run(context.clone()).await.unwrap();

    println!("accepted response: {:?}", result.response);
    println!(
        "f1 vs. reference (one word differs: \"the\" vs \"a\"): {:?}",
        context.get::<f64>("eval::f1_score")
    );
}
