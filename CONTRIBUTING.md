# Contributing

```bash
git clone https://github.com/thaicn1712/agentflow
cd agentflow
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

All three must pass before a PR is merged — CI runs the same checks. This crate is mostly re-exports plus `pipeline::AgentOpsTask`; changes to the underlying behavior of orchestration/guarding/validation/evaluation belong in `graphflow-stream`, `guardflow`, `schemaflow`, or `evalflow` respectively, not here.
