//! Chat RAG e2e (mock LLM) latency benchmark (#144 v1.1).
//!
//! 量化 attune chat 端到端检索 + 注入路径的算法层 latency（不含 LLM 调用本身）。
//! mock LLM 时延 = 0（criterion 不测网络/远程模型），focus 在 attune **本地**做的事：
//!
//!   1. query embedding（确定性 word-hash pseudo-embedding，省去 embedding model 依赖）
//!   2. 向量检索（top-K，HNSW 模拟 = 线性扫描小 corpus）
//!   3. 全文检索（BM25 简化 = unigram TF count）
//!   4. RRF fusion（attune-core::search::rrf_fuse）
//!   5. budget allocation（attune-core::search::allocate_budget）
//!   6. prompt 拼装（chunk inject_content concat）
//!
//! 这是产品 SLO 「chat 首字时延」的本地路径成本（LLM round-trip 时延独立测）。
//!
//! Corpus sizes：
//!   - small (50 docs)
//!   - medium (500 docs)
//!   - large (5000 docs)
//!
//! 跑法：cargo bench --bench chat_e2e
//! 报告：target/criterion/chat_e2e/

use attune_core::search::{allocate_budget, rrf_fuse, SearchResult, INJECTION_BUDGET};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

const TOP_K: usize = 8;

/// 确定性 pseudo-embedding（avoid LLM dep）：char-byte hashing → 32-d vector。
fn word_hash_embedding(text: &str) -> [f32; 32] {
    let mut v = [0f32; 32];
    for (i, ch) in text.chars().enumerate() {
        let bucket = (ch as usize) % 32;
        v[bucket] += 1.0 + (i as f32 * 0.01); // 位置权重微调
    }
    // L2 normalize
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
    v
}

fn cosine(a: &[f32; 32], b: &[f32; 32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// 简化 BM25：unigram TF count，无 IDF（避免 dep + 数据集 prep）
fn bm25_score(query: &str, doc: &str) -> f32 {
    let qt: Vec<&str> = query.split_whitespace().collect();
    qt.iter().filter(|t| doc.contains(*t)).count() as f32
}

struct DocFixture {
    id: String,
    content: String,
    embedding: [f32; 32],
}

fn synthesize_corpus(n: usize) -> Vec<DocFixture> {
    let topics = [
        "RAG 检索增强生成",
        "vector embedding 向量化",
        "BM25 全文检索",
        "RRF 融合排序",
        "Argon2id 密码派生",
        "AES-GCM 加密",
        "HDBSCAN 聚类",
        "chunking 文档分块",
        "tantivy 中文分词",
        "知识库 vault 加密",
    ];
    (0..n)
        .map(|i| {
            let topic = topics[i % topics.len()];
            let content = format!(
                "{} 是 attune 项目的核心算法之一。本文档 #{} 详细介绍了该技术的实现原理和性能特性。包含代码示例与 benchmark 数据。",
                topic, i
            );
            let embedding = word_hash_embedding(&content);
            DocFixture {
                id: format!("doc-{}", i),
                content,
                embedding,
            }
        })
        .collect()
}

/// 端到端 chat 检索路径（mock LLM = 不发起远程调用）
fn chat_e2e_retrieve(query: &str, corpus: &[DocFixture]) -> Vec<SearchResult> {
    // 阶段 1: query embedding
    let q_emb = word_hash_embedding(query);

    // 阶段 2: vector top-K（线性扫描，小 corpus 足够；真生产用 HNSW）
    let mut vec_scored: Vec<(String, f32)> = corpus
        .iter()
        .map(|d| (d.id.clone(), cosine(&q_emb, &d.embedding)))
        .collect();
    vec_scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let vec_results: Vec<(String, f32)> = vec_scored.into_iter().take(50).collect();

    // 阶段 3: BM25 top-K（unigram TF）
    let mut bm_scored: Vec<(String, f32)> = corpus
        .iter()
        .map(|d| (d.id.clone(), bm25_score(query, &d.content)))
        .collect();
    bm_scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let bm_results: Vec<(String, f32)> = bm_scored.into_iter().take(50).collect();

    // 阶段 4: RRF fusion（attune-core 生产 API）
    let fused = rrf_fuse(&vec_results, &bm_results, 0.6, 0.4, TOP_K);

    // 阶段 5: 转 SearchResult + 阶段 6 budget allocate
    let mut results: Vec<SearchResult> = fused
        .iter()
        .map(|(id, score)| {
            let content = corpus
                .iter()
                .find(|d| &d.id == id)
                .map(|d| d.content.clone())
                .unwrap_or_default();
            SearchResult {
                item_id: id.clone(),
                score: *score,
                title: String::new(),
                content,
                source_type: "doc".into(),
                inject_content: None,
                corpus_domain: "general".into(),
                ..Default::default()
            }
        })
        .collect();
    allocate_budget(&mut results, INJECTION_BUDGET);
    results
}

fn bench_chat_e2e_corpus_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("chat_e2e_corpus_size");
    let query = "RAG 检索 attune RRF 融合";
    for &n in &[50usize, 500, 5000] {
        let corpus = synthesize_corpus(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &corpus, |b, c| {
            b.iter(|| {
                let _ = chat_e2e_retrieve(black_box(query), black_box(c));
            });
        });
    }
    group.finish();
}

fn bench_chat_e2e_top_k(c: &mut Criterion) {
    let mut group = c.benchmark_group("chat_e2e_top_k_500docs");
    let corpus = synthesize_corpus(500);
    let query = "vector embedding cosine 向量";
    group.bench_function("retrieve_top8_inject_budget_default", |b| {
        b.iter(|| {
            let results = chat_e2e_retrieve(black_box(query), black_box(&corpus));
            // 报告平均 inject content size（marketing claim 用）
            let _total_chars: usize = results
                .iter()
                .map(|r| r.inject_content.as_deref().unwrap_or("").len())
                .sum();
        });
    });
    group.finish();
}

criterion_group!(benches, bench_chat_e2e_corpus_size, bench_chat_e2e_top_k);
criterion_main!(benches);
