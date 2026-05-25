# attune-bench

[![Nightly Benchmark](https://github.com/qiurui144/attune-bench/actions/workflows/nightly-bench.yml/badge.svg)](https://github.com/qiurui144/attune-bench/actions/workflows/nightly-bench.yml)

Quantitative benchmarks for **attune** algorithm advantages.

## Bench 矩阵

### v1.0 base bench(算法优势对照,#131)

vs naive baselines:

- **token_savings** — attune chunking + RRF vs paste-everything prompt size
- **retrieval_accuracy** — RRF vs BM25 vs cosine recall@K / nDCG
- **encrypt_overhead** — 字段加密 (AES-256-GCM + Argon2id) vs plaintext mode
- **hdbscan_accuracy** — cluster purity (adjusted rand index / NMI)

### v1.1 扩展 bench(产品 SLO 算法层,#144)

attune v1.0 GA + 14-agent plugin pack 的算法层 latency 维度:

- **agent_dispatch** — 14 agent dispatcher routing + budget 决策 latency(纯 CPU)
- **vault_unlock** — Argon2id KDF + AES-GCM 完整 unlock flow 用户感知 latency
- **chat_e2e** — RAG 检索 + 注入路径(query embedding + BM25 + RRF + budget),mock LLM
- **plugin_install** — YAML parse + manifest validate + SHA-256 verify pipeline

每 bench 至少 2-3 个 size variant(small / medium / large),criterion HTML 含 violin /
flame / regression analysis。

> **不在 scope**:LLM 推理 throughput(由 `vlm-llm-benchmark` 测)、真 subprocess
> spawn latency(由 attune 主仓 e2e harness 测)、真 plugin pack ZIP 解压(产品集成
> 测试覆盖)。本仓专注**纯算法层**对照数据。

Independent from `attune` main repo to avoid polluting product code + decouple
release cycle.

## Run

```bash
# v1.0 base
cargo bench --bench token_savings
cargo bench --bench encrypt_overhead
cargo bench --bench retrieval_accuracy
cargo bench --bench hdbscan_accuracy

# v1.1 new
cargo bench --bench agent_dispatch
cargo bench --bench vault_unlock
cargo bench --bench chat_e2e
cargo bench --bench plugin_install

# 全跑
cargo bench
```

HTML reports land in `target/criterion/<bench>/report/index.html`。

## v1.0.0 baseline data

2026-05-23 跑出的算法基线见 [`docs/v1.0-baseline.md`](docs/v1.0-baseline.md)。

criterion HTML 报告(本地交互式 flamechart + violin plot)持久化在
[`docs/criterion-html-v1.0/report/index.html`](docs/criterion-html-v1.0/report/index.html)
(checkout 后用浏览器打开)。

CI 每次运行也会上传 `criterion-report-<sha>` artifact(Actions → workflow run → Artifacts)。

主要数据点:

| Claim | 实测值 | 来源 |
|-------|--------|------|
| 长文档 prompt 节省 ≥ 95% | 100 KB doc saved **96.9%** / 500 KB saved **99.4%** | token_savings |
| 字段加密 overhead < 5% | 1 KB encrypt **1.4 µs** vs DB INSERT 1-5 ms = **< 0.1%** | encrypt_overhead |
| Argon2id 抗暴力 ≥ 100 ms | **114.5 ms** | encrypt_overhead |
| RRF 不 hurt BM25 baseline | Hit@10 + MRR 持平 | retrieval_accuracy |
| HDBSCAN 聚类 ARI ≥ 0.5 | **0.5786** (8-d synthetic) | hdbscan_accuracy |

环境:Intel i9-14900K / Ubuntu 24.04 / Rust 1.95.0 / attune-core SHA `d74b0ee`。

## v1.1 baseline data

待 nightly CI 跑出后填写,占位见 [`RELEASE.md`](RELEASE.md) v1.1.0 节。

## CI / 历史趋势追踪

nightly CI(`.github/workflows/nightly-bench.yml`)每天 04:00 UTC 自动跑全部 8 bench:

- 上传 `criterion-report-<sha>` artifact(retention 90 天)
- 上传 `bench-logs-<sha>` artifact(retention 30 天)
- (best-effort)推送 historical trend 到 `gh-pages` 分支供 [github-action-benchmark](https://github.com/benchmark-action/github-action-benchmark) 渲染

启用 GH Pages dashboard 前需在 repo Settings → Pages 选 `gh-pages` 分支作为
source(初次推送时 workflow `continue-on-error` 不阻塞)。

手动趋势对比工作流:

1. 每次跑完把 `docs/criterion-html-v1.X/` 更新 commit(带版本标签)
2. 关键指标变化手动记录在 `docs/v1.X-baseline.md`(追加新版本节)
3. 更结构化需求接入 [bencher.dev](https://bencher.dev) 或 GH Pages dashboard

## Repo layout

```
attune-bench/
├── benches/
│   ├── token_savings.rs       # v1.0
│   ├── encrypt_overhead.rs    # v1.0
│   ├── retrieval_accuracy.rs  # v1.0
│   ├── hdbscan_accuracy.rs    # v1.0
│   ├── agent_dispatch.rs      # v1.1
│   ├── vault_unlock.rs        # v1.1
│   ├── chat_e2e.rs            # v1.1
│   └── plugin_install.rs      # v1.1
├── docs/
│   ├── v1.0-baseline.md         # v1.0 数据
│   └── criterion-html-v1.0/     # v1.0 HTML snapshot
├── .github/workflows/
│   └── nightly-bench.yml      # nightly CI
├── Cargo.toml      # criterion harness + attune-core path dep
├── README.md
├── DEVELOP.md      # 维护者文档
└── RELEASE.md      # 版本历史 SSOT
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
