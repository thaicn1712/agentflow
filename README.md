# agentflow

DevOps automates software delivery. MLOps adds data and model lifecycle. LLMOps adds prompts, grounding, and hallucination control. **AgentOps** is the next layer down: running autonomous agent graphs in production needs the same discipline — orchestration, guardrails, data validation, evaluation — applied to *agents*, not just models.

`agentflow` is that stack for Rust, in one crate: four independent crates, each solving a gap Python's ecosystem has and Rust's didn't, composed into one AgentOps lifecycle.

| Stage | Crate | What it does |
|---|---|---|
| **Orchestrate** | [`graphflow-stream`](https://crates.io/crates/graphflow-stream) | Token-level streaming for [`graph-flow`](https://crates.io/crates/graph-flow) agent graphs |
| **Guard** | [`guardflow`](https://crates.io/crates/guardflow) | Validators + retry-until-valid guard for LLM output |
| **Validate** | [`schemaflow`](https://crates.io/crates/schemaflow) | Pandera-style schema validation for Polars DataFrames |
| **Evaluate** | [`evalflow`](https://crates.io/crates/evalflow) | DeepEval-style graded-score test suites |

Each is a standalone crate you can `cargo add` on its own. `agentflow` re-exports all four under one dependency, plus glue code for the parts of the lifecycle that genuinely benefit from knowing about each other.

## Install

```bash
cargo add agentflow
```

## Usage

Re-exports — use any of the four exactly as documented in their own READMEs:

```rust,ignore
use agentflow::{stream, guard, schema, eval};

let (rx, handle) = stream::spawn_task(my_task, context, 32);
let g = guard::Guard::new().with(guard::validators::NotEmpty);
```

The glue: `AgentOpsTask<T>` wraps any `graph_flow::Task` with both a guard (hard pass/fail, retried) and metrics (soft graded scores, non-blocking) — the guard→evaluate half of the lifecycle in one task, with scores written into the shared `Context` so anything downstream can read them:

```rust,ignore
use agentflow::pipeline::AgentOpsTask;
use agentflow::guard::Guard;
use agentflow::guard::validators::NotEmpty;
use agentflow::eval::metrics::F1Score;

let task = AgentOpsTask::new(my_task)
    .with_guard(Guard::new().with(NotEmpty))
    .with_max_attempts(3)
    .with_metric(F1Score::default());

let result = task.run(context.clone()).await?;
let f1: Option<f64> = context.get("eval::f1_score");
```

## Why one crate instead of four

Each crate is deliberately independent — no crate requires another, so you're never forced into the whole stack. `agentflow` exists for the case where you want all four without hunting down four separate `cargo add`s, and for the parts of the lifecycle (guard + evaluate together, in this v0.1) where composing them by hand would just be boilerplate.

## License

MIT
