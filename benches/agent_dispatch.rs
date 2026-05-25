//! Agent dispatch latency benchmark (#144 v1.1).
//!
//! 量化 attune 14-agent dispatcher 的 routing + budget 决策算法层 latency。
//! **不真跑** 任何 agent subprocess / LLM call — 只测 dispatcher 自身路由决策
//! 的纯 CPU latency（关键词匹配 + chat_trigger 路由 + token budget 计算）。
//!
//! 测的是 dispatcher 决策层时延（产品 perf SLO 的子集）：
//!   - small batch (1 user query → 1 agent 决策)
//!   - medium batch (10 queries → 10 routing 决策)
//!   - large batch (100 queries → 100 routing 决策)
//!
//! 14 agent 模拟 = attune-pro law-pro 4 + traffic/divorce/sale/housing/defamation
//! 5 + extractor 5（self-contained，不调 attune-core）。
//!
//! 注：真实端到端 agent dispatch latency（含 subprocess spawn + LLM call）由
//! `agent_golden_gate.rs` + e2e harness 测，**不**在 criterion bench scope。
//! 本 bench 专注 dispatcher 路由 + 资源分配的算法层成本（用户输入 → 选定 agent）。
//!
//! 跑法：cargo bench --bench agent_dispatch
//! 报告：target/criterion/agent_dispatch/

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::collections::HashMap;

/// 单个 agent 描述（dispatcher 路由用）。
#[derive(Clone)]
struct AgentDescriptor {
    id: &'static str,
    /// chat_trigger 关键词集（命中即视作候选）。
    keywords: Vec<&'static str>,
    /// 优先级（数字大 = 优先）。
    priority: u32,
    /// 平均 token cost 估算（注入预算分配用）。
    avg_tokens: u32,
}

/// 14 agent fixture（per attune-pro law-pro 现役 + 新 agent，模拟）。
fn fixture_agents() -> Vec<AgentDescriptor> {
    vec![
        AgentDescriptor {
            id: "law-pro-contract",
            keywords: vec!["合同", "contract", "约定", "条款"],
            priority: 10,
            avg_tokens: 1200,
        },
        AgentDescriptor {
            id: "law-pro-divorce",
            keywords: vec!["离婚", "财产分割", "抚养"],
            priority: 12,
            avg_tokens: 1500,
        },
        AgentDescriptor {
            id: "law-pro-traffic-accident",
            keywords: vec!["交通事故", "碰撞", "责任认定"],
            priority: 11,
            avg_tokens: 1400,
        },
        AgentDescriptor {
            id: "law-pro-housing-rent",
            keywords: vec!["租赁", "房屋", "押金", "退租"],
            priority: 9,
            avg_tokens: 1100,
        },
        AgentDescriptor {
            id: "law-pro-sale-contract",
            keywords: vec!["买卖", "货款", "交付"],
            priority: 9,
            avg_tokens: 1100,
        },
        AgentDescriptor {
            id: "law-pro-defamation",
            keywords: vec!["名誉", "诽谤", "侮辱"],
            priority: 8,
            avg_tokens: 1000,
        },
        AgentDescriptor {
            id: "law-pro-inheritance",
            keywords: vec!["继承", "遗嘱", "遗产"],
            priority: 8,
            avg_tokens: 1000,
        },
        AgentDescriptor {
            id: "extractor-contract",
            keywords: vec!["提取", "extract", "条款抽取"],
            priority: 5,
            avg_tokens: 800,
        },
        AgentDescriptor {
            id: "extractor-divorce",
            keywords: vec!["离婚", "财产清单"],
            priority: 5,
            avg_tokens: 800,
        },
        AgentDescriptor {
            id: "extractor-defamation",
            keywords: vec!["名誉", "证据"],
            priority: 5,
            avg_tokens: 800,
        },
        AgentDescriptor {
            id: "extractor-traffic",
            keywords: vec!["事故", "现场"],
            priority: 5,
            avg_tokens: 800,
        },
        AgentDescriptor {
            id: "extractor-housing",
            keywords: vec!["租约", "房屋信息"],
            priority: 5,
            avg_tokens: 800,
        },
        AgentDescriptor {
            id: "general-search",
            keywords: vec![],
            priority: 1,
            avg_tokens: 600,
        },
        AgentDescriptor {
            id: "general-chat",
            keywords: vec![],
            priority: 1,
            avg_tokens: 500,
        },
    ]
}

/// Dispatcher 决策（关键词命中 + 优先级 + budget 校验）。
/// 返回入选 agent id + 分配的 token budget。
fn dispatch(query: &str, agents: &[AgentDescriptor], total_budget: u32) -> Option<(String, u32)> {
    // 阶段 1：关键词路由（命中候选集）
    let candidates: Vec<&AgentDescriptor> = agents
        .iter()
        .filter(|a| {
            if a.keywords.is_empty() {
                false // general fallback 在阶段 2 兜底
            } else {
                a.keywords.iter().any(|k| query.contains(k))
            }
        })
        .collect();

    // 阶段 2：选优先级最高的；若无候选 → general fallback
    let chosen = if candidates.is_empty() {
        agents.iter().filter(|a| a.id.starts_with("general-")).max_by_key(|a| a.priority)?
    } else {
        *candidates.iter().max_by_key(|a| a.priority)?
    };

    // 阶段 3：budget 校验（avg_tokens 不能超过总 budget 的 80%）
    let allocated = if chosen.avg_tokens as f64 > total_budget as f64 * 0.8 {
        (total_budget as f64 * 0.8) as u32
    } else {
        chosen.avg_tokens
    };

    Some((chosen.id.to_string(), allocated))
}

fn synthetic_queries(n: usize) -> Vec<String> {
    let templates = [
        "我和对方签的合同对方违约怎么办",
        "离婚后财产怎么分割",
        "交通事故责任认定后赔偿",
        "租房押金到期不退怎么办",
        "买卖合同对方不交货",
        "网上别人发了诽谤我的内容",
        "爷爷的遗产继承顺序",
        "请帮我提取这份合同的关键条款",
        "如何评估我的案件",
        "搜索 RAG 实现",
        "今天天气怎么样",
    ];
    (0..n)
        .map(|i| templates[i % templates.len()].to_string())
        .collect()
}

fn bench_dispatch_single(c: &mut Criterion) {
    let agents = fixture_agents();
    let mut group = c.benchmark_group("agent_dispatch_single");
    let query = "我和对方签的合同对方违约怎么办";
    group.bench_function("single_query_14_agents", |b| {
        b.iter(|| {
            let _ = dispatch(black_box(query), black_box(&agents), 4000);
        });
    });
    group.finish();
}

fn bench_dispatch_batch(c: &mut Criterion) {
    let agents = fixture_agents();
    let mut group = c.benchmark_group("agent_dispatch_batch");
    for &n in &[1usize, 10, 100] {
        let queries = synthetic_queries(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &queries, |b, qs| {
            b.iter(|| {
                for q in qs {
                    let _ = dispatch(black_box(q), &agents, 4000);
                }
            });
        });
    }
    group.finish();
}

fn bench_dispatch_hit_distribution(c: &mut Criterion) {
    let agents = fixture_agents();
    // 跑一次统计命中分布（marketing claim：14 agent 路由准确率）
    let queries = synthetic_queries(100);
    let mut hits: HashMap<String, u32> = HashMap::new();
    for q in &queries {
        if let Some((id, _)) = dispatch(q, &agents, 4000) {
            *hits.entry(id).or_insert(0) += 1;
        }
    }
    eprintln!("[agent_dispatch] 100 queries hit distribution:");
    let mut sorted: Vec<_> = hits.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));
    for (id, n) in sorted {
        eprintln!("  {}: {}", id, n);
    }

    let mut group = c.benchmark_group("agent_dispatch_distribution");
    group.bench_function("dispatch_100_queries", |b| {
        b.iter(|| {
            for q in &queries {
                let _ = dispatch(black_box(q), &agents, 4000);
            }
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_dispatch_single,
    bench_dispatch_batch,
    bench_dispatch_hit_distribution
);
criterion_main!(benches);
