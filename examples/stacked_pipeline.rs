//! Stack rollback (checkpoint), trace (cost/latency), and cache (skip repeat
//! calls) around one task by nesting `graph_flow::Task` wrappers — no glue
//! code needed, each crate only knows about `graph_flow` itself.

use agentflow::{cache, rollback, trace};
use async_trait::async_trait;
use graph_flow::{Context, NextAction, Task, TaskResult, error::Result};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct SimulatedLlmCall {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl Task for SimulatedLlmCall {
    fn id(&self) -> &str {
        "call_llm"
    }

    async fn run(&self, context: Context) -> Result<TaskResult> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let prompt: String = context.get("prompt").unwrap_or_default();
        trace::report_usage(120, 40, 0.002).await;
        Ok(TaskResult::new(
            Some(format!("answer to: {prompt}")),
            NextAction::Continue,
        ))
    }
}

#[tokio::main]
async fn main() {
    let calls = Arc::new(AtomicUsize::new(0));
    let tracer = Arc::new(trace::InMemoryTracer::default());
    let checkpoint_store: Arc<dyn rollback::CheckpointStore> =
        Arc::new(rollback::InMemoryCheckpointStore::default());

    let task = rollback::CheckpointedTask::new(
        trace::TracedTask::new(
            cache::CachedTask::new(
                SimulatedLlmCall {
                    calls: calls.clone(),
                },
                cache::InMemoryCache::default(),
                |ctx: &Context| ctx.get::<String>("prompt").unwrap_or_default(),
            ),
            tracer.clone(),
        ),
        checkpoint_store,
        "session-1",
    );

    let context = Context::new();
    context.set("prompt", "what is agentflow?").unwrap();

    task.run(context.clone()).await.unwrap();
    task.run(context).await.unwrap(); // identical prompt: cache hit, inner never runs again

    println!(
        "inner LLM task actually ran: {} time(s)",
        calls.load(Ordering::SeqCst)
    );
    println!(
        "total cost recorded by trace across both calls: ${:.4}",
        tracer.total_cost_usd()
    );
}
