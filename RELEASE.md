# RELEASE — attune-bench

attune-bench 版本历史 SSOT。每个 release 节包含 Highlights / Bench 变更 / Baseline
数据(GA 后回填)/ Known Limitations。

## v1.1.0 — 数据扩 + nightly CI + GH Pages dashboard(预占,TBD 2026-08)

**主题**:配合 attune v1.0 GA + 14-agent plugin pack,补齐 dispatcher / vault unlock /
chat e2e / plugin install 算法层 latency 维度;CI 升级到 nightly。

### Highlights

- 4 个新 bench 框架(算法层 latency 维度):
  - `agent_dispatch` — 14-agent dispatcher routing + budget 决策 latency(纯 CPU)
  - `vault_unlock` — Argon2id KDF + AES-GCM 完整 unlock flow 用户感知 latency
  - `chat_e2e` — RAG 检索 + 注入路径(query embedding + BM25 + RRF + budget),mock LLM
  - `plugin_install` — YAML parse + manifest validate + SHA-256 verify pipeline
- CI:`bench.yml` 升级为 `nightly-bench.yml`,nightly 04:00 UTC schedule
- 性能退化告警:github-action-benchmark 集成,> 150% 触发 issue comment(不 fail build)
- GH Pages dashboard(best-effort,初次需 repo Settings → Pages 启用 gh-pages 分支)
- 新依赖(dev-only):`serde`,`serde_yaml`,`sha2`(plugin_install bench 用)

### Bench 变更

| Bench | 状态 | 维度 | 备注 |
|-------|------|-----|------|
| token_savings | 保留(v1.0) | doc_size: 10K / 100K / 500K | v1.0 baseline 沿用 |
| encrypt_overhead | 保留(v1.0) | payload_size: 1K / 10K / 100K + KDF | v1.0 baseline 沿用 |
| retrieval_accuracy | 保留(v1.0) | 20 doc + 8 query | v1.0 baseline 沿用 |
| hdbscan_accuracy | 保留(v1.0) | 8-d × 160 点 | v1.0 baseline 沿用 |
| agent_dispatch | **新增** | single / batch 1/10/100 | dispatcher 路由 + budget 决策 |
| vault_unlock | **新增** | metadata: 256B / 1KB / 4KB + KDF-only | 完整 unlock flow 用户感知 latency |
| chat_e2e | **新增** | corpus: 50 / 500 / 5000 doc | RAG 全链(mock LLM) |
| plugin_install | **新增** | binary_size: 64K / 1M / 4M + full pipeline | YAML + SHA-256 + validate |

### Baseline 数据(待 GA 后回填)

待 nightly CI 跑出 v1.1 baseline 数据后,补到 `docs/v1.1-baseline.md`。占位:

| Claim | 目标 | 实测值 | 来源 |
|-------|------|--------|------|
| Agent dispatcher 14-agent 单 query 决策 | < 50 µs | TBD | agent_dispatch |
| Vault unlock 用户感知 latency | < 300 ms (KDF dominant) | TBD | vault_unlock |
| Chat e2e 500 doc corpus 检索 + 注入 | < 50 ms | TBD | chat_e2e |
| Plugin install 14-agent 14 MB pack | < 500 ms | TBD | plugin_install |

### Known Limitations

- 4 个新 bench 当前是**算法层 framework**(self-contained mock),**不**真触发 attune
  14-agent subprocess / 真 LLM call / 真 ZIP 解压。这些 e2e latency 由 attune 主仓
  `agent_golden_gate.rs` + e2e harness 测,**不在 attune-bench scope**
- `agent_dispatch` fixture 14 agent 是模拟描述符,真实 attune-pro law-pro plugin 的
  dispatcher 决策可能含更多业务规则(本 bench 测路由 + budget 的纯算法层成本)
- `chat_e2e` mock LLM 时延 = 0,真实 chat 首字时延 = chat_e2e bench 数据 + LLM
  round-trip(后者独立测)
- `plugin_install` 不真解压 ZIP 不真 spawn binary,只测 YAML + SHA-256 + validate
- attune-core path dep 写死本地路径,CI 自动 sed 换 git dep,external user 跑前需
  手动改 `Cargo.toml`(或等 v1.2 改造成 published crate)
- GH Pages dashboard 首次需 repo Settings 启用 + 推一次 master 才能渲染

### Migration

无 breaking change,v1.0 4 个 bench 全部保留;新增 4 个 bench 不影响现有数据采集。

---

## v1.0.0 — Quantitative algorithm baseline(2026-05-23 GA)

**主题**:attune 4 个核心算法优势 vs naive baseline 的量化 benchmark。

### Highlights

- 4 algorithm bench(criterion-driven, statistical):
  - `token_savings` — attune chunking + RRF vs paste-everything prompt size
  - `encrypt_overhead` — AES-256-GCM 字段加密 vs plaintext + Argon2id KDF
  - `retrieval_accuracy` — RRF vs BM25 vs cosine recall@K / nDCG
  - `hdbscan_accuracy` — cluster purity (adjusted rand index / NMI)
- criterion HTML report 持久化 docs/criterion-html-v1.0/
- 独立仓维护,不污染 attune 主仓 cargo build cache
- Weekly CI bench.yml(Mon 03:00 UTC)

### Baseline 数据(已测)

| Claim | 实测值 | 来源 |
|-------|--------|------|
| 长文档 prompt 节省 ≥ 95% | 100 KB doc saved **96.9%** / 500 KB saved **99.4%** | token_savings |
| 字段加密 overhead < 5% | 1 KB encrypt **1.4 µs** vs DB INSERT 1-5 ms = **< 0.1%** | encrypt_overhead |
| Argon2id 抗暴力 ≥ 100 ms | **114.5 ms** | encrypt_overhead |
| RRF 不 hurt BM25 baseline | Hit@10 + MRR 持平 | retrieval_accuracy |
| HDBSCAN 聚类 ARI ≥ 0.5 | **0.5786** (8-d synthetic) | hdbscan_accuracy |

环境:Intel i9-14900K / Ubuntu 24.04 / Rust 1.95.0 / attune-core SHA `d74b0ee`。

### Known Limitations

- 不测 LLM 推理 throughput(交给 `vlm-llm-benchmark` 仓)
- 不测产品 e2e 性能(交给 attune 主仓集成测试)
- baseline 数据基于 Intel i9-14900K,其他硬件需各自跑 + 提交到 community-baselines

---

## 历史(pre-1.0)

- `initial attune-bench` — 4 bench 初版(token_savings + retrieval_accuracy + encrypt_overhead + hdbscan_accuracy),迁出 attune 主仓 benches/
