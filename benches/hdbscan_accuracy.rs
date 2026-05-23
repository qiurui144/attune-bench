//! HDBSCAN cluster accuracy benchmark (#122 v1.1).
//!
//! 在已知 ground-truth label 的合成 embedding corpus 上跑 HDBSCAN,
//! 计算两个 cluster quality 指标:
//!   - **Adjusted Rand Index (ARI)** [-1, 1], 1 = 完美匹配 truth, 0 = 随机, 负数比随机更差
//!   - **Purity** [0, 1], top cluster label 占该 cluster 多数比例 mean
//!
//! 合成 corpus:
//!   - 5 簇 × 每簇 30 个 8-d embedding (gaussian cluster, σ=0.15, centers spread)
//!   - 加 10 个 noise 点 (uniform random)
//!   - 总计 160 点
//!
//! 确定性 — 用 LCG 随机数, fixed seed, 每次跑同样输出. 不引入 rand 依赖额外 feature.
//!
//! Note: `Clusterer` 需要 LLM 才能 name cluster, bench 只测 `run_hdbscan` 等价的
//! algorithm path (hdbscan 直接调用). Cluster naming 是 LLM-bound, 不在 bench scope.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::collections::HashMap;

const DIM: usize = 8;
const POINTS_PER_CLUSTER: usize = 30;
const NUM_CLUSTERS: usize = 5;
const NUM_NOISE: usize = 10;
const SIGMA: f32 = 0.15;
const SEED: u32 = 42;

/// LCG random, deterministic.
struct Lcg {
    state: u64,
}
impl Lcg {
    fn new(seed: u32) -> Self {
        Self {
            state: (seed as u64).wrapping_mul(2654435761) | 1,
        }
    }
    fn next_f32(&mut self) -> f32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (((self.state >> 40) as u32) as f32) / ((1u64 << 24) as f32)
    }
    /// Box-Muller gaussian ~ N(0, 1)
    fn next_gauss(&mut self) -> f32 {
        let u1 = self.next_f32().max(1e-9);
        let u2 = self.next_f32();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos()
    }
}

/// 合成 corpus → (Vec<embedding>, Vec<true_label>)
fn build_synthetic_corpus() -> (Vec<Vec<f32>>, Vec<i32>) {
    let mut rng = Lcg::new(SEED);
    let mut embs = Vec::new();
    let mut labels = Vec::new();
    // 5 cluster centers, spread on 8-d unit sphere-ish
    let centers: Vec<Vec<f32>> = (0..NUM_CLUSTERS)
        .map(|c| {
            let mut center = vec![0.0; DIM];
            for (d, val) in center.iter_mut().enumerate() {
                *val = if (c + d) % 3 == 0 { 1.0 } else { -0.2 };
            }
            let norm: f32 = center.iter().map(|x| x * x).sum::<f32>().sqrt();
            for v in &mut center {
                *v /= norm;
            }
            center
        })
        .collect();
    for (c, center) in centers.iter().enumerate() {
        for _ in 0..POINTS_PER_CLUSTER {
            let mut p = vec![0.0; DIM];
            for (d, x) in p.iter_mut().enumerate() {
                *x = center[d] + SIGMA * rng.next_gauss();
            }
            let norm: f32 = p.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for v in &mut p {
                    *v /= norm;
                }
            }
            embs.push(p);
            labels.push(c as i32);
        }
    }
    // noise: 全随机 uniform 球面
    for _ in 0..NUM_NOISE {
        let mut p = vec![0.0; DIM];
        for x in p.iter_mut() {
            *x = rng.next_gauss();
        }
        let norm: f32 = p.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut p {
                *v /= norm;
            }
        }
        embs.push(p);
        labels.push(-1);
    }
    (embs, labels)
}

/// Adjusted Rand Index. noise (-1) 算作独立 class.
fn adjusted_rand_index(truth: &[i32], pred: &[i32]) -> f64 {
    assert_eq!(truth.len(), pred.len());
    let n = truth.len();
    if n < 2 {
        return 0.0;
    }
    let mut table: HashMap<(i32, i32), usize> = HashMap::new();
    let mut a_sum: HashMap<i32, usize> = HashMap::new();
    let mut b_sum: HashMap<i32, usize> = HashMap::new();
    for (t, p) in truth.iter().zip(pred.iter()) {
        *table.entry((*t, *p)).or_insert(0) += 1;
        *a_sum.entry(*t).or_insert(0) += 1;
        *b_sum.entry(*p).or_insert(0) += 1;
    }
    let comb2 = |x: usize| -> f64 {
        if x < 2 { 0.0 } else { (x * (x - 1)) as f64 / 2.0 }
    };
    let sum_nij: f64 = table.values().map(|&v| comb2(v)).sum();
    let sum_ai: f64 = a_sum.values().map(|&v| comb2(v)).sum();
    let sum_bj: f64 = b_sum.values().map(|&v| comb2(v)).sum();
    let total_pairs = comb2(n);
    if total_pairs == 0.0 {
        return 0.0;
    }
    let expected = sum_ai * sum_bj / total_pairs;
    let max = 0.5 * (sum_ai + sum_bj);
    if (max - expected).abs() < 1e-12 {
        return 0.0;
    }
    (sum_nij - expected) / (max - expected)
}

/// Purity: 每个 pred cluster 找内部 truth majority 占比, 按 cluster size 加权.
fn purity(truth: &[i32], pred: &[i32]) -> f64 {
    let n = truth.len();
    let mut groups: HashMap<i32, Vec<i32>> = HashMap::new();
    for (t, p) in truth.iter().zip(pred.iter()) {
        groups.entry(*p).or_default().push(*t);
    }
    let mut sum_majority = 0.0;
    for (cluster, members) in &groups {
        if *cluster == -1 {
            continue;
        }
        let mut counts: HashMap<i32, usize> = HashMap::new();
        for t in members {
            *counts.entry(*t).or_insert(0) += 1;
        }
        let max_count = counts.values().copied().max().unwrap_or(0);
        sum_majority += max_count as f64;
    }
    if n == 0 {
        0.0
    } else {
        sum_majority / n as f64
    }
}

fn run_hdbscan(embs: &[Vec<f32>]) -> Vec<i32> {
    let data: Vec<Vec<f32>> = embs.to_vec();
    let clusterer = hdbscan::Hdbscan::default_hyper_params(&data);
    clusterer
        .cluster()
        .expect("hdbscan should not fail on synthetic corpus")
}

fn report_metrics() {
    let (embs, truth) = build_synthetic_corpus();
    let pred = run_hdbscan(&embs);
    let ari = adjusted_rand_index(&truth, &pred);
    let pur = purity(&truth, &pred);
    let n_clusters_pred = {
        let mut s: std::collections::BTreeSet<i32> = pred.iter().copied().collect();
        s.remove(&-1);
        s.len()
    };
    let n_noise_pred = pred.iter().filter(|&&l| l == -1).count();
    eprintln!(
        "[hdbscan_accuracy] N={} truth_clusters={} truth_noise={}",
        embs.len(),
        NUM_CLUSTERS,
        NUM_NOISE
    );
    eprintln!("  pred_clusters={n_clusters_pred} pred_noise={n_noise_pred}");
    eprintln!("  Adjusted Rand Index: {ari:.4}   Purity: {pur:.4}");
}

fn bench_hdbscan(c: &mut Criterion) {
    report_metrics();
    let mut group = c.benchmark_group("hdbscan");
    group.sample_size(20);
    let (embs, _truth) = build_synthetic_corpus();
    group.bench_function("160points_8dim_5clusters", |b| {
        b.iter(|| {
            let _labels = run_hdbscan(black_box(&embs));
        });
    });
    group.finish();
}

criterion_group!(benches, bench_hdbscan);
criterion_main!(benches);
