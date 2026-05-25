# DEVELOP — attune-bench

attune-bench 维护者文档。本仓 8 个 criterion bench(v1.0 base 4 + v1.1 扩 4),
跟随 attune 主仓 release 节奏出 baseline data,接受外部 PR 加新 bench / 改进算法
对照实现。

## 本地开发

### 依赖

- Rust stable(本仓不依赖 nightly feature)
- 本地 attune 主仓 clone(默认 `/data/company/project/attune/rust/crates/attune-core` —
  本仓 `Cargo.toml` path dep);CI 自动 sed 替换为 git dep。

### 跑单个 bench

```bash
cargo bench --bench token_savings
# HTML 报告:target/criterion/token_savings/report/index.html
```

### 跑全部

```bash
cargo bench
```

⚠️ Argon2id KDF + chat_e2e 大 corpus 等 bench 跑一次 ~30 s,全跑约 5-10 min。
CI(nightly)开 60 min timeout。

### 加新 bench 流程

1. 在 `benches/<name>.rs` 写 bench(criterion `harness = false` pattern)
2. 在 `Cargo.toml` 加 `[[bench]] name = "<name>" harness = false`
3. 在 `.github/workflows/nightly-bench.yml` 加 `Run <name> bench` step
4. 在 `README.md` 加描述 + 数据点(待 baseline 出来后回填)
5. 在 `RELEASE.md` 当前版本节加 changelog 一行

**算法层 vs 集成层划线**:本仓只测**纯 CPU 算法层**(确定性 input → 确定性 output 的
算法成本)。**不**测:LLM round-trip、subprocess spawn、网络 IO、磁盘 IO、UI render。
这些属产品 e2e 测试范畴。

### 风格约定

- bench 文件顶部必须有 module doc comment(`//!`)说明:测什么、不测什么、跑法、报告位置
- 用 `criterion::black_box` 包裹热路径输入,防止编译器 const-fold
- `eprintln!` 报告业务指标(saved_pct / Hit@K 等),criterion 本身只测时间
- 不引入新 production dep,只加 `[dev-dependencies]`
- mock / synthetic corpus 用确定性算法(LCG 随机 + fixed seed),避免 CI 跑出不同数

## CI / Release

### CI workflow

`.github/workflows/nightly-bench.yml`:

- nightly 04:00 UTC schedule + `workflow_dispatch` 手动触发 + master push trigger
- 顺序跑 8 bench(v1.0 base 4 + v1.1 new 4)
- 上传 criterion HTML artifact(90 天 retention)+ bench logs(30 天)
- best-effort 推 historical trend 到 `gh-pages` 分支(via github-action-benchmark)

CI 用 git dep:

```toml
attune-core = { git = "https://github.com/qiurui144/attune", branch = "develop" }
```

workflow 自动 sed 替换 `path = "..."` → `git = "..."`,不需要手动改 Cargo.toml。

### Release 节奏

attune-bench 不强绑 attune 主仓 SemVer,但建议跟随 attune 大版本节奏:

| attune-bench | 触发 | 主要内容 |
|--------------|------|---------|
| v1.0.0 | attune v1.0 GA 前 | 4 algorithm bench(token / encrypt / retrieval / hdbscan)+ baseline data |
| v1.1.0 | attune v1.0 GA 后 | 4 new bench(agent_dispatch / vault_unlock / chat_e2e / plugin_install)+ nightly CI |
| v1.2.0 | 后续按需 | 新 algorithm 优势 / 新 baseline 对照 |

打 tag 流程:

1. 跑 `cargo bench` 在固定环境(Intel i9-14900K / Ubuntu 24.04 / Rust stable),
   收集 baseline data 写进 `docs/vX.Y-baseline.md`
2. commit + push 到 master
3. `git tag attune-bench-vX.Y.Z` + `git push origin attune-bench-vX.Y.Z`
4. GH Releases 页粘 `RELEASE.md` 对应版本节

### GH Pages dashboard(可选)

首次启用:

1. repo Settings → Pages → Source 选 `gh-pages` 分支
2. 等 nightly CI 首次推 `gh-pages/bench-history/` 数据
3. 访问 `https://qiurui144.github.io/attune-bench/bench-history/` 看 trend graph

未启用时 workflow `continue-on-error: true` 跳过,不影响 artifact 上传。

## 测试矩阵约束

本仓**不**跑产品级 e2e 测试。但 bench 本身需要保证 `cargo bench --bench <X>` 在
clean environment 跑得起来:

- 无网络依赖(synthetic corpus inline 在 source)
- 无外部模型依赖(pseudo-embedding 用 word-hash)
- 无大文件 fixture(全部 inline 或程序生成)
- attune-core 通过 path 或 git dep,不依赖 attune 主仓 build artifact

## 外部 PR 接入

接受外部贡献:

1. **新 baseline data**:跑 attune-bench 在不同硬件上,提交结果到 `docs/community-baselines/<arch>.md`
2. **新 algorithm bench**:对照新 algorithm vs naive baseline,先开 issue 讨论是否进 v1.X
3. **CI 改进**:GH Pages dashboard / bencher.dev 集成 / 矩阵 OS 测试

## 与 attune 主仓的契约

- 本仓**不**修改 attune 主仓代码,只**调用** `attune-core::{search, crypto, chunker, ...}`
  public API
- attune-core API breaking change 会让本仓 CI 跑炸 → attune 主仓应主动同步本仓 PR
  适配(workflow:attune 主仓改 API → attune-bench PR 同步)
- bench baseline 数据是 attune 产品 marketing claim 的事实依据
  (per `docs/benchmarks/README.md` in attune 主仓)
