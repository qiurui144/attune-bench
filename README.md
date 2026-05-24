# attune-bench

[![Benchmark](https://github.com/qiurui144/attune-bench/actions/workflows/bench.yml/badge.svg)](https://github.com/qiurui144/attune-bench/actions/workflows/bench.yml)

Quantitative benchmarks for **attune** algorithm advantages.

Measures (vs naive baselines):

- **token_savings** — attune chunking + RRF vs paste-everything prompt size
- **retrieval_accuracy** — RRF vs BM25 vs cosine recall@K / nDCG
- **encrypt_overhead** — 字段加密 (AES-256-GCM + Argon2id) vs plaintext mode
- **hdbscan_accuracy** — cluster purity (adjusted rand index / NMI)

Independent from `attune` main repo to avoid polluting product code + decouple
release cycle.

## Run

```bash
cargo bench --bench token_savings
cargo bench --bench encrypt_overhead
cargo bench --bench retrieval_accuracy
cargo bench --bench hdbscan_accuracy
```

HTML reports land in `target/criterion/<bench>/report/index.html`.

## v1.0.0 baseline data

2026-05-23 跑出的算法基线见 [`docs/v1.0-baseline.md`](docs/v1.0-baseline.md)。

criterion HTML 报告（本地交互式 flamechart + violin plot）持久化在
[`docs/criterion-html-v1.0/report/index.html`](docs/criterion-html-v1.0/report/index.html)（checkout 后用浏览器打开）。

CI 每次运行也会上传 `criterion-report-<sha>` artifact（Actions → workflow run → Artifacts）。

主要数据点：

| Claim | 实测值 | 来源 |
|-------|--------|------|
| 长文档 prompt 节省 ≥ 95% | 100 KB doc saved **96.9%** / 500 KB saved **99.4%** | token_savings |
| 字段加密 overhead < 5% | 1 KB encrypt **1.4 µs** vs DB INSERT 1-5 ms = **< 0.1%** | encrypt_overhead |
| Argon2id 抗暴力 ≥ 100 ms | **114.5 ms** | encrypt_overhead |
| RRF 不 hurt BM25 baseline | Hit@10 + MRR 持平 | retrieval_accuracy |
| HDBSCAN 聚类 ARI ≥ 0.5 | **0.5786** (8-d synthetic) | hdbscan_accuracy |

环境：Intel i9-14900K / Ubuntu 24.04 / Rust 1.95.0 / attune-core SHA `d74b0ee`。

## Historical trend tracking

criterion 本身不跨 run 做 trend graph。推荐工作流：

1. 每次跑完把 `docs/criterion-html-v1.0/` 更新 commit（带版本标签，如 `docs/criterion-html-v1.1/`）
2. 关键指标变化手动记录在 `docs/v1.0-baseline.md`（追加新版本节）
3. 更结构化需求可接入 [bencher.dev](https://bencher.dev) 或 [github-action-benchmark](https://github.com/benchmark-action/github-action-benchmark)（当前不强依赖）

## Repo layout

```
attune-bench/
├── benches/
│   ├── token_savings.rs
│   ├── encrypt_overhead.rs
│   ├── retrieval_accuracy.rs
│   └── hdbscan_accuracy.rs
├── docs/
│   ├── v1.0-baseline.md         # quantitative baseline 文字数据
│   └── criterion-html-v1.0/     # criterion HTML 交互报告（v1.0 snapshot）
└── Cargo.toml      # criterion harness + attune-core path dep
```

## Related repos

- **attune main** — https://github.com/qiurui144/attune (product code)
- **attune-bench** (this) — https://github.com/qiurui144/attune-bench (algorithm benchmarks)
- **vlm-llm-benchmark** — https://github.com/qiurui144/vlm-llm-benchmark (measures MODEL throughput / accuracy, not attune algorithm — different layer, do not mix)

## Why a separate repo

per 5/23 决策 — bench harness 不进产品仓:

1. 避免污染 attune 主仓产品代码 (benches/ 拖累 cargo build cache)
2. 独立 release 周期 (algorithm benchmark 不绑定产品版本)
3. 公开可比 (open-source benchmark suite, 接受外部 PR 加 baseline)

## License

Apache-2.0
