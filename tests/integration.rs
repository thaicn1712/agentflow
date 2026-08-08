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

#[test]
fn act_guard_reexport_is_wired_up_and_fails_closed() {
    use agentflow::act_guard::policies::AllowList;
    use agentflow::act_guard::{Decision, PolicySet, ToolCall};

    let policies = PolicySet::new().with(AllowList::new(["read_file"]));
    assert_eq!(
        policies.check(&ToolCall::new("read_file", serde_json::json!({}))),
        Decision::Allow
    );
    assert!(matches!(
        policies.check(&ToolCall::new("shell_exec", serde_json::json!({}))),
        Decision::Deny(_)
    ));
}

#[tokio::test]
async fn rollback_trace_and_cache_reexports_stack_around_one_task() {
    use agentflow::{cache, rollback, trace};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Echo {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Task for Echo {
        fn id(&self) -> &str {
            "echo"
        }

        async fn run(&self, context: Context) -> Result<TaskResult> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let prompt: String = context.get("prompt").unwrap_or_default();
            trace::report_usage(10, 5, 0.001).await;
            Ok(TaskResult::new(Some(prompt), NextAction::Continue))
        }
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let tracer = Arc::new(trace::InMemoryTracer::default());
    let checkpoint_store: Arc<dyn rollback::CheckpointStore> =
        Arc::new(rollback::InMemoryCheckpointStore::default());

    let task = rollback::CheckpointedTask::new(
        trace::TracedTask::new(
            cache::CachedTask::new(
                Echo {
                    calls: calls.clone(),
                },
                cache::InMemoryCache::default(),
                |ctx: &Context| ctx.get::<String>("prompt").unwrap_or_default(),
            ),
            tracer.clone(),
        ),
        checkpoint_store,
        "session-x",
    );

    let context = Context::new();
    context.set("prompt", "hi").unwrap();

    task.run(context.clone()).await.unwrap();
    task.run(context).await.unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!((tracer.total_cost_usd() - 0.001).abs() < 1e-9);
}

#[tokio::test]
async fn memory_reexport_injects_retrieved_memories() {
    use agentflow::memory;
    use std::sync::Arc;

    struct WordCountEmbedder;

    #[async_trait]
    impl memory::Embedder for WordCountEmbedder {
        async fn embed(&self, text: &str) -> Vec<f32> {
            vec![text.split_whitespace().count() as f32, text.len() as f32]
        }
    }

    struct Echo;

    #[async_trait]
    impl Task for Echo {
        fn id(&self) -> &str {
            "echo"
        }

        async fn run(&self, context: Context) -> Result<TaskResult> {
            let message: String = context.get("message").unwrap_or_default();
            Ok(TaskResult::new(Some(message), NextAction::Continue))
        }
    }

    let store: Arc<dyn memory::MemoryStore> =
        Arc::new(memory::InMemoryStore::new(WordCountEmbedder));
    store.add("user likes concise answers".to_string()).await;

    let task = memory::MemoryTask::new(
        Echo,
        store,
        |_ctx: &Context| "user likes concise answers".to_string(),
        memory::AlwaysAdd,
    );

    let context = Context::new();
    context.set("message", "hello").unwrap();
    task.run(context.clone()).await.unwrap();

    let retrieved: Vec<String> = context.get(memory::RETRIEVED_KEY).unwrap();
    assert_eq!(retrieved, vec!["user likes concise answers".to_string()]);
}
