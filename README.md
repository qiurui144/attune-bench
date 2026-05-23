# attune-bench

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

主要数据点：

| Claim | 实测值 | 来源 |
|-------|--------|------|
| 长文档 prompt 节省 ≥ 95% | 100 KB doc saved **96.9%** / 500 KB saved **99.4%** | token_savings |
| 字段加密 overhead < 5% | 1 KB encrypt **1.4 µs** vs DB INSERT 1-5 ms = **< 0.1%** | encrypt_overhead |
| Argon2id 抗暴力 ≥ 100 ms | **114.5 ms** | encrypt_overhead |
| RRF 不 hurt BM25 baseline | Hit@10 + MRR 持平 | retrieval_accuracy |
| HDBSCAN 聚类 ARI ≥ 0.5 | **0.5786** (8-d synthetic) | hdbscan_accuracy |

环境：Intel i9-14900K / Ubuntu 24.04 / Rust 1.95.0 / attune-core SHA `d74b0ee`。

## Repo layout

```
attune-bench/
├── benches/
│   ├── token_savings.rs
│   ├── encrypt_overhead.rs
│   ├── retrieval_accuracy.rs
│   └── hdbscan_accuracy.rs
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
