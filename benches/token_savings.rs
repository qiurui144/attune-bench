//! Token savings benchmark (#122 v1.1).
//!
//! 量化 attune 检索注入 vs naive "paste 全文" 的 prompt size 节省。
//!
//! 算法：
//!   1. 取一个 100 KB 模拟文档（多章节 markdown）
//!   2. naive baseline = 整个文档作为 prompt context
//!   3. attune path = chunker::extract_sections + chunk → top-K（按一个 query 的 BM25-ish
//!      heuristic 评分）→ 注入 budget 截断
//!   4. 报告 saved_pct = 1 - attune_tokens / naive_tokens
//!
//! 注意 — 这是**算法层**节省，不是 e2e LLM token count（避免依赖 tokenizer 实测）。
//! 用 char count 近似 token count（中文 ~1 char/token，英文 ~4 chars/token，
//! 这里中英混杂语料校准为 ~3 chars/token 经验值，在 docs/benchmarks 报告里说明）。
//!
//! 跑法：cargo bench -p attune-core --bench token_savings
//! 报告：rust/target/criterion/token_savings/

use attune_core::chunker;
use attune_core::search::{allocate_budget, rrf_fuse, SearchResult, INJECTION_BUDGET};
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};

/// 生成模拟 100 KB 文档（多章节 markdown，中英混排）。
fn synthetic_doc(target_size: usize) -> String {
    let mut s = String::with_capacity(target_size);
    let mut chapter = 1;
    while s.len() < target_size {
        s.push_str(&format!(
            "# Chapter {chapter}: 知识库构建实战\n\n这一章主要讨论 vector embedding 与 hybrid retrieval 的工程实现。\n\n"
        ));
        for sect in 1..=4 {
            s.push_str(&format!(
                "## {chapter}.{sect} 子章节：实际案例\n\n以下段落覆盖 BM25 / cosine similarity / RRF 融合策略，配合实际代码示例帮助读者理解。"
            ));
            for _ in 0..6 {
                s.push_str("段落内容：分块策略对 RAG 检索质量影响重大，512 字符 chunk size 在 bge-m3 模型上接近最优。Code-fence 边界保持也很关键，不切到中间。 ");
            }
            s.push_str("\n\n```rust\nfn retrieve(query: &str) -> Vec<Chunk> {\n    let vec_hits = vectors.search(query, 50);\n    let bm25_hits = fulltext.search(query, 50);\n    rrf_fuse(&vec_hits, &bm25_hits, 0.6, 0.4, 10)\n}\n```\n\n");
        }
        chapter += 1;
    }
    s.truncate(target_size);
    s
}

/// Naive baseline: 整篇文档作为 prompt（char count = token count proxy）。
fn naive_prompt_size(doc: &str) -> usize {
    doc.chars().count()
}

/// Attune path: extract_sections → chunk → 模拟 retrieval → budget allocate
/// 返回最终 inject 的 char count（含 chunk header overhead）。
fn attune_inject_size(doc: &str, top_k: usize, budget: usize) -> usize {
    // 1. 章节切分（J1）
    let sections = chunker::extract_sections_with_path(doc);
    // 2. 段落级 chunk
    let mut all_chunks: Vec<(String, String)> = Vec::new(); // (id, text)
    for (sect_idx, sect) in sections.iter().enumerate() {
        let chunks = chunker::chunk(&sect.content, 512, 128);
        for (chunk_idx, c) in chunks.into_iter().enumerate() {
            all_chunks.push((format!("s{sect_idx}c{chunk_idx}"), c));
        }
    }
    // 3. 模拟 vector + fulltext 排序（简化：按 chunk 出现顺序作 vector ranking，
    //    按反向作 fulltext ranking 模拟两个不同视角）
    let n = all_chunks.len();
    let vec_results: Vec<(String, f32)> = (0..n)
        .map(|i| (all_chunks[i].0.clone(), 1.0 - i as f32 / n as f32))
        .collect();
    let fts_results: Vec<(String, f32)> = (0..n)
        .map(|i| {
            (
                all_chunks[n - 1 - i].0.clone(),
                1.0 - i as f32 / n as f32,
            )
        })
        .collect();
    // 4. RRF 融合
    let fused = rrf_fuse(&vec_results, &fts_results, 0.6, 0.4, top_k);
    // 5. 转 SearchResult + budget allocate
    let mut results: Vec<SearchResult> = fused
        .iter()
        .map(|(id, score)| {
            let content = all_chunks
                .iter()
                .find(|(cid, _)| cid == id)
                .map(|(_, t)| t.clone())
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
    allocate_budget(&mut results, budget);
    results
        .iter()
        .map(|r| r.inject_content.as_deref().unwrap_or("").chars().count())
        .sum()
}

fn bench_token_savings(c: &mut Criterion) {
    let mut group = c.benchmark_group("token_savings");
    // 测 3 个文档大小（典型工程文档 10 KB / 长文 100 KB / 巨型手册 500 KB）。
    for &doc_size in &[10_240usize, 102_400, 512_000] {
        let doc = synthetic_doc(doc_size);
        let naive_chars = naive_prompt_size(&doc);
        // attune path（top-8 + budget 2000）— budget 来自 search::INJECTION_BUDGET
        let attune_chars = attune_inject_size(&doc, 8, INJECTION_BUDGET);
        let saved = 1.0 - (attune_chars as f64 / naive_chars as f64);
        // eprintln 让人在 cargo bench 输出里也能看到 ratio（criterion 主要测时间，
        // 但 saved% 是这个 bench 的真业务指标）
        eprintln!(
            "[token_savings] doc_size={doc_size} bytes naive={naive_chars} chars \
             attune={attune_chars} chars saved={:.1}%",
            saved * 100.0
        );

        group.throughput(Throughput::Bytes(doc_size as u64));
        // bench attune retrieval-build path 的时间（不含 LLM 调用，这是算法层 cost）。
        let bench_name = format!("attune_path_{doc_size}B");
        group.bench_function(&bench_name, |b| {
            b.iter(|| {
                let _size = attune_inject_size(black_box(&doc), 8, INJECTION_BUDGET);
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_token_savings);
criterion_main!(benches);
