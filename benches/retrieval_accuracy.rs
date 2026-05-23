//! Retrieval accuracy benchmark (#122 v1.1).
//!
//! 对比 attune 三种检索模式在同一 corpus + ground-truth 上的质量指标：
//!   - **BM25 only** (tantivy FulltextIndex)
//!   - **cosine only** (确定性 word-hash pseudo-embedding，避免外部 LLM 依赖)
//!   - **RRF fused** (BM25 + cosine，与 attune 生产路径一致：`search::rrf_fuse`)
//!
//! 指标：Hit@5 / Hit@10 / Recall@10 / MRR
//!
//! Ground truth：内联 mini-corpus（20 doc + 8 query），每 query 标 2-3 acceptable_hits。
//! 不依赖外部 corpus 下载，保证 CI 任何环境可跑（per spec §F 真 corpus 跑数另走
//! `scripts/run-benchmark-corpus.sh` + attune-server e2e，不进 criterion bench）。
//!
//! 输出：
//!   - criterion HTML：rust/target/criterion/retrieval_accuracy/
//!   - eprintln 指标对比（marketing claim 数据源）

use attune_core::index::FulltextIndex;
use attune_core::search::rrf_fuse;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::collections::{HashMap, HashSet};

/// 单 doc：(id, title, content)
type Doc = (&'static str, &'static str, &'static str);

/// Mini corpus：20 doc 跨 RAG / Rust / 加密 / 法律 4 主题，
/// 让 BM25 vs cosine 有可对比的覆盖差异。
fn mini_corpus() -> Vec<Doc> {
    vec![
        // RAG 主题（5 doc）
        ("rag-1", "RRF 融合介绍", "Reciprocal Rank Fusion 是混合检索中常用的排序融合算法。"),
        ("rag-2", "Hybrid Search Strategy", "Combining BM25 lexical search with vector cosine similarity yields better recall."),
        ("rag-3", "RAG 系统的 chunking 策略", "分块大小 512 字符对 bge-m3 模型最优，overlap 128。"),
        ("rag-4", "向量检索召回阈值", "cosine 0.65 平衡召回与精度，吴师兄文章实测 0.62 → 0.91 提升。"),
        ("rag-5", "Cross-encoder Reranker", "Reranking top-50 candidates with a cross-encoder improves NDCG by 5-10%."),
        // Rust 主题（5 doc）
        ("rust-1", "Ownership and Borrowing", "Rust's ownership system guarantees memory safety without garbage collection."),
        ("rust-2", "Lifetimes 详解", "生命周期是 Rust 类型系统的核心，标注 'a 表示引用有效作用域。"),
        ("rust-3", "Async Tokio Runtime", "Tokio is the de facto async runtime for Rust, supporting work-stealing scheduling."),
        ("rust-4", "Cargo Workspaces", "Workspaces let you share Cargo.lock and target/ across multiple related crates."),
        ("rust-5", "Trait Objects vs Generics", "Generics offer zero-cost abstraction; trait objects allow dynamic dispatch."),
        // 加密 / 安全（5 doc）
        ("sec-1", "AES-GCM 认证加密", "AES-256-GCM provides confidentiality + authenticity via 96-bit nonce + 128-bit tag."),
        ("sec-2", "Argon2id 密码哈希", "Argon2id 是 RFC 9106 推荐的密码 KDF，抗 GPU/ASIC 暴力破解。"),
        ("sec-3", "Zero Trust Architecture", "Zero trust assumes no implicit trust based on network location."),
        ("sec-4", "字段级加密 vs 全盘加密", "Field-level encryption protects individual columns; full-disk protects everything at rest."),
        ("sec-5", "Key Rotation Policy", "Periodic key rotation limits blast radius of compromised keys."),
        // 法律（5 doc）
        ("law-1", "劳动合同解除规定", "劳动者解除劳动合同应当提前 30 日以书面形式通知用人单位。"),
        ("law-2", "民间借贷利率上限", "民间借贷利率不得超过合同成立时一年期 LPR 的四倍。"),
        ("law-3", "商标侵权法律责任", "商标侵权需承担停止侵权、赔偿损失等民事责任。"),
        ("law-4", "公司股东会决议程序", "股东会决议须经代表过半数表决权的股东通过。"),
        ("law-5", "合同违约金调整", "民法典第 585 条规定违约金过高或过低可请求法院调整。"),
    ]
}

/// Query + ground-truth acceptable hits。
struct Query {
    id: &'static str,
    text: &'static str,
    acceptable: &'static [&'static str],
}

fn queries() -> Vec<Query> {
    vec![
        // RAG queries
        Query { id: "q1", text: "RRF 融合算法", acceptable: &["rag-1", "rag-2"] },
        Query { id: "q2", text: "chunking 大小最佳实践", acceptable: &["rag-3"] },
        Query { id: "q3", text: "vector cosine threshold", acceptable: &["rag-4", "rag-2"] },
        // Rust queries
        Query { id: "q4", text: "Rust lifetime annotation", acceptable: &["rust-2", "rust-1"] },
        Query { id: "q5", text: "tokio async runtime", acceptable: &["rust-3"] },
        // Sec queries
        Query { id: "q6", text: "AES GCM nonce 长度", acceptable: &["sec-1"] },
        Query { id: "q7", text: "密码哈希推荐算法", acceptable: &["sec-2"] },
        // Law queries
        Query { id: "q8", text: "劳动合同解除提前通知", acceptable: &["law-1"] },
    ]
}

/// 确定性 pseudo-embedding：词袋 → hash 到固定维度向量。
/// 不是真 semantic embedding，但提供"词级" cosine 视角（与 lexical 同时存在的次维度）。
/// 这样 cosine path 与 BM25 path 在词重叠时同好，在 stem / 同义词时一样差 —
/// 这是 fair：纯 lexical 模型的 token-level 视角。
fn pseudo_embed(text: &str, dim: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; dim];
    // 简易"分词"：unicode 字符 + ASCII 词。
    // 中文按 char 算 token，英文按 whitespace。
    let mut tokens: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        // 含中文 → 拆 char；否则整词
        if word.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)) {
            for c in word.chars() {
                tokens.push(c.to_string());
            }
        } else {
            tokens.push(word.to_lowercase());
        }
    }
    for t in &tokens {
        let h = hash_str(t) as usize;
        let idx = h % dim;
        v[idx] += 1.0;
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

fn hash_str(s: &str) -> u64 {
    // FNV-1a 64
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// 3 模式各跑一遍，返回 (mode_name → metrics) 的 map。
struct Metrics {
    hit_at_5: f64,
    hit_at_10: f64,
    recall_at_10: f64,
    mrr: f64,
}

fn evaluate(retrieved_per_query: &[Vec<String>], queries: &[Query]) -> Metrics {
    let n = queries.len();
    let mut sum_hit5 = 0.0;
    let mut sum_hit10 = 0.0;
    let mut sum_recall10 = 0.0;
    let mut sum_mrr = 0.0;
    for (q, retrieved) in queries.iter().zip(retrieved_per_query.iter()) {
        let acceptable: HashSet<&str> = q.acceptable.iter().copied().collect();
        // Hit@K
        let hit5 = retrieved.iter().take(5).any(|id| acceptable.contains(id.as_str())) as u32;
        let hit10 = retrieved.iter().take(10).any(|id| acceptable.contains(id.as_str())) as u32;
        sum_hit5 += hit5 as f64;
        sum_hit10 += hit10 as f64;
        // Recall@10
        let hits_in_10 = retrieved
            .iter()
            .take(10)
            .filter(|id| acceptable.contains(id.as_str()))
            .count();
        sum_recall10 += hits_in_10 as f64 / acceptable.len() as f64;
        // MRR
        let mut rr = 0.0;
        for (rank, id) in retrieved.iter().enumerate() {
            if acceptable.contains(id.as_str()) {
                rr = 1.0 / (rank + 1) as f64;
                break;
            }
        }
        sum_mrr += rr;
    }
    Metrics {
        hit_at_5: sum_hit5 / n as f64,
        hit_at_10: sum_hit10 / n as f64,
        recall_at_10: sum_recall10 / n as f64,
        mrr: sum_mrr / n as f64,
    }
}

fn build_corpus_index() -> (FulltextIndex, HashMap<String, Vec<f32>>) {
    let docs = mini_corpus();
    let ft = FulltextIndex::open_memory().expect("open_memory");
    let mut emb_by_id: HashMap<String, Vec<f32>> = HashMap::new();
    for (id, title, content) in &docs {
        ft.add_document(id, title, content, "doc")
            .expect("add_document");
        let combined = format!("{title} {content}");
        emb_by_id.insert(id.to_string(), pseudo_embed(&combined, 256));
    }
    (ft, emb_by_id)
}

fn retrieve_bm25(ft: &FulltextIndex, query: &str, top_k: usize) -> Vec<String> {
    ft.search(query, top_k)
        .map(|hits| hits.into_iter().map(|(id, _)| id).collect())
        .unwrap_or_default()
}

fn retrieve_cosine(
    emb_by_id: &HashMap<String, Vec<f32>>,
    query: &str,
    top_k: usize,
) -> Vec<String> {
    let q_emb = pseudo_embed(query, 256);
    let mut scored: Vec<(String, f32)> = emb_by_id
        .iter()
        .map(|(id, e)| (id.clone(), cosine(&q_emb, e)))
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(top_k);
    scored.into_iter().map(|(id, _)| id).collect()
}

fn retrieve_rrf(
    ft: &FulltextIndex,
    emb_by_id: &HashMap<String, Vec<f32>>,
    query: &str,
    top_k: usize,
) -> Vec<String> {
    let bm25 = ft.search(query, 50).unwrap_or_default();
    let q_emb = pseudo_embed(query, 256);
    let mut cos: Vec<(String, f32)> = emb_by_id
        .iter()
        .map(|(id, e)| (id.clone(), cosine(&q_emb, e)))
        .collect();
    cos.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    cos.truncate(50);
    let fused = rrf_fuse(&cos, &bm25, 0.6, 0.4, top_k);
    fused.into_iter().map(|(id, _)| id).collect()
}

fn run_eval_all_modes() -> (Metrics, Metrics, Metrics) {
    let (ft, emb_by_id) = build_corpus_index();
    let qs = queries();
    let bm25_results: Vec<Vec<String>> = qs.iter().map(|q| retrieve_bm25(&ft, q.text, 10)).collect();
    let cos_results: Vec<Vec<String>> = qs.iter().map(|q| retrieve_cosine(&emb_by_id, q.text, 10)).collect();
    let rrf_results: Vec<Vec<String>> = qs.iter().map(|q| retrieve_rrf(&ft, &emb_by_id, q.text, 10)).collect();
    let m_bm25 = evaluate(&bm25_results, &qs);
    let m_cos = evaluate(&cos_results, &qs);
    let m_rrf = evaluate(&rrf_results, &qs);
    (m_bm25, m_cos, m_rrf)
}

fn print_metrics_table(bm25: &Metrics, cos: &Metrics, rrf: &Metrics) {
    eprintln!("[retrieval_accuracy] mini corpus 20 doc / 8 query");
    eprintln!(
        "  mode      Hit@5    Hit@10   Recall@10   MRR"
    );
    eprintln!(
        "  BM25      {:.3}    {:.3}    {:.3}       {:.3}",
        bm25.hit_at_5, bm25.hit_at_10, bm25.recall_at_10, bm25.mrr
    );
    eprintln!(
        "  cosine    {:.3}    {:.3}    {:.3}       {:.3}",
        cos.hit_at_5, cos.hit_at_10, cos.recall_at_10, cos.mrr
    );
    eprintln!(
        "  RRF       {:.3}    {:.3}    {:.3}       {:.3}",
        rrf.hit_at_5, rrf.hit_at_10, rrf.recall_at_10, rrf.mrr
    );
    // 优势 % vs BM25 baseline
    let rrf_gain_hit10 = if bm25.hit_at_10 > 0.0 {
        (rrf.hit_at_10 - bm25.hit_at_10) / bm25.hit_at_10 * 100.0
    } else {
        0.0
    };
    let rrf_gain_mrr = if bm25.mrr > 0.0 {
        (rrf.mrr - bm25.mrr) / bm25.mrr * 100.0
    } else {
        0.0
    };
    eprintln!(
        "  RRF vs BM25:  Hit@10 {:+.1}%   MRR {:+.1}%",
        rrf_gain_hit10, rrf_gain_mrr
    );
}

fn bench_retrieval_modes(c: &mut Criterion) {
    // 一次性算 + print 指标对比（这是 bench 的真业务输出）。
    let (m_bm25, m_cos, m_rrf) = run_eval_all_modes();
    print_metrics_table(&m_bm25, &m_cos, &m_rrf);

    let mut group = c.benchmark_group("retrieval");
    group.sample_size(50);
    let (ft, emb_by_id) = build_corpus_index();
    let qs = queries();

    group.bench_function("bm25_8queries", |b| {
        b.iter(|| {
            for q in &qs {
                let _ = retrieve_bm25(black_box(&ft), black_box(q.text), 10);
            }
        });
    });
    group.bench_function("cosine_8queries", |b| {
        b.iter(|| {
            for q in &qs {
                let _ = retrieve_cosine(black_box(&emb_by_id), black_box(q.text), 10);
            }
        });
    });
    group.bench_function("rrf_8queries", |b| {
        b.iter(|| {
            for q in &qs {
                let _ = retrieve_rrf(
                    black_box(&ft),
                    black_box(&emb_by_id),
                    black_box(q.text),
                    10,
                );
            }
        });
    });
    group.finish();
}

criterion_group!(benches, bench_retrieval_modes);
criterion_main!(benches);
