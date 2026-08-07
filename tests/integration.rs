use std::sync::atomic::{AtomicUsize, Ordering};

use agentflow::eval::metrics::F1Score;
use agentflow::guard::Guard;
use agentflow::guard::validators::MinLength;
use agentflow::pipeline::{AgentOpsTask, EXPECTED_OUTPUT_KEY};
use async_trait::async_trait;
use graph_flow::{Context, NextAction, Task, TaskResult, error::Result};

struct FlakyTask {
    attempts: AtomicUsize,
    succeeds_on: usize,
}

#[async_trait]
impl Task for FlakyTask {
    fn id(&self) -> &str {
        "flaky"
    }

    async fn run(&self, _context: Context) -> Result<TaskResult> {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
        let response = if attempt >= self.succeeds_on {
            "this response is long enough to pass the guard"
        } else {
            "short"
        };
        Ok(TaskResult::new(
            Some(response.to_string()),
            NextAction::Continue,
        ))
    }
}

#[tokio::test]
async fn guard_retries_then_metrics_score_the_accepted_response() {
    let inner = FlakyTask {
        attempts: AtomicUsize::new(0),
        succeeds_on: 2,
    };
    let task = AgentOpsTask::new(inner)
        .with_guard(Guard::new().with(MinLength(20)))
        .with_max_attempts(5)
        .with_metric(F1Score::default());

    let context = Context::new();
    context
        .set(
            EXPECTED_OUTPUT_KEY,
            "this response is long enough to pass the guard",
        )
        .unwrap();
    let result = task.run(context.clone()).await.unwrap();

    assert_eq!(
        result.response,
        Some("this response is long enough to pass the guard".to_string())
    );

    let score: Option<f64> = context.get("eval::f1_score");
    assert_eq!(score, Some(1.0));
}

#[tokio::test]
async fn without_an_expected_output_set_metrics_still_run_but_score_low() {
    let inner = FlakyTask {
        attempts: AtomicUsize::new(0),
        succeeds_on: 1,
    };
    let task = AgentOpsTask::new(inner).with_metric(F1Score::default());

    let context = Context::new();
    task.run(context.clone()).await.unwrap();

    let score: Option<f64> = context.get("eval::f1_score");
    assert_eq!(score, Some(0.0));
}

#[tokio::test]
async fn without_a_guard_the_task_runs_once_and_is_still_scored() {
    struct Echo;

    #[async_trait]
    impl Task for Echo {
        fn id(&self) -> &str {
            "echo"
        }

        async fn run(&self, _context: Context) -> Result<TaskResult> {
            Ok(TaskResult::new(
                Some("hello".to_string()),
                NextAction::Continue,
            ))
        }
    }

    let task = AgentOpsTask::new(Echo).with_metric(F1Score::default());
    let context = Context::new();
    let result = task.run(context.clone()).await.unwrap();

    assert_eq!(result.response, Some("hello".to_string()));
    assert!(context.get::<f64>("eval::f1_score").is_some());
}
