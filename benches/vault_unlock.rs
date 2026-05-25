//! Vault unlock latency benchmark (#144 v1.1).
//!
//! 测 attune vault **完整 unlock flow** 时延（端到端用户感知 latency）：
//!   1. Argon2id KDF 派生 master key（一次性 ~100-200 ms 抗暴力 cost）
//!   2. master key → 一次 AES-256-GCM decrypt（解 vault 元数据 blob）
//!   3. (optional) 一次 round-trip encrypt + decrypt 校验
//!
//! 与现有 `encrypt_overhead` 区别：
//!   - encrypt_overhead 单独测 KDF + 不同 payload size 的 encrypt / decrypt 时延
//!   - vault_unlock 测 **完整 unlock 一次** 的端到端 wall-clock latency（用户视角）
//!
//! 用户感知 latency = 整个 unlock 链路（按 enter password → vault 可用）。
//! 目标：< 300 ms (Argon2id 抗暴力 ≥ 100 ms + decrypt overhead < 10 ms)
//!
//! 跑法：cargo bench --bench vault_unlock
//! 报告：target/criterion/vault_unlock/

use attune_core::crypto;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

/// 模拟 vault metadata blob（unlock 时第一次 decrypt 的对象）。
/// 真实 attune vault metadata ~512 bytes（项目偏好 + recent search 等）。
fn vault_metadata(size: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(size);
    // 字符串字面量（含中文 UTF-8），再取 bytes。byte string literal `b"..."`
    // 不允许非 ASCII 字符。
    let template = "{\"version\":1,\"recent\":[\"项目A\",\"项目B\"],\"prefs\":{\"theme\":\"dark\"}}".as_bytes();
    while buf.len() < size {
        buf.extend_from_slice(template);
    }
    buf.truncate(size);
    buf
}

fn bench_unlock_full_flow(c: &mut Criterion) {
    let mut group = c.benchmark_group("vault_unlock_full");
    // unlock 是低频操作（不在热路径），但 latency 直接影响用户感知。
    group.sample_size(10);

    let password = b"correct horse battery staple";
    let device_secret = [0u8; 32];
    let salt = crypto::generate_salt();
    // Pre-encrypt vault metadata（512 bytes 典型 size）。
    let metadata = vault_metadata(512);
    let pre_key = crypto::derive_master_key(password, &device_secret, &salt).unwrap();
    let encrypted_blob = crypto::encrypt(&pre_key, &metadata).unwrap();

    group.bench_function("unlock_512B_metadata", |b| {
        b.iter(|| {
            // 阶段 1：Argon2id KDF（dominant cost ~100-200 ms）
            let mk = crypto::derive_master_key(
                black_box(password),
                black_box(&device_secret),
                black_box(&salt),
            )
            .unwrap();
            // 阶段 2：decrypt metadata blob
            let _plain = crypto::decrypt(black_box(&mk), black_box(&encrypted_blob)).unwrap();
        });
    });
    group.finish();
}

fn bench_unlock_different_metadata_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("vault_unlock_metadata_size");
    group.sample_size(10);

    let password = b"correct horse battery staple";
    let device_secret = [0u8; 32];
    let salt = crypto::generate_salt();
    let pre_key = crypto::derive_master_key(password, &device_secret, &salt).unwrap();

    // small (256 B) / medium (1 KB) / large (4 KB) metadata blob — 测 decrypt 占比
    for &size in &[256usize, 1024, 4096] {
        let metadata = vault_metadata(size);
        let blob = crypto::encrypt(&pre_key, &metadata).unwrap();
        group.bench_with_input(BenchmarkId::from_parameter(size), &blob, |b, blob| {
            b.iter(|| {
                let mk = crypto::derive_master_key(
                    black_box(password),
                    black_box(&device_secret),
                    black_box(&salt),
                )
                .unwrap();
                let _plain = crypto::decrypt(black_box(&mk), black_box(blob)).unwrap();
            });
        });
    }
    group.finish();
}

fn bench_unlock_kdf_isolated(c: &mut Criterion) {
    // 独立测 KDF 阶段，便于和 unlock_full_flow 对比 KDF 占比
    let mut group = c.benchmark_group("vault_unlock_kdf_only");
    group.sample_size(10);

    let password = b"correct horse battery staple";
    let device_secret = [0u8; 32];
    let salt = crypto::generate_salt();
    group.bench_function("kdf_only", |b| {
        b.iter(|| {
            let _mk = crypto::derive_master_key(
                black_box(password),
                black_box(&device_secret),
                black_box(&salt),
            )
            .unwrap();
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_unlock_full_flow,
    bench_unlock_different_metadata_sizes,
    bench_unlock_kdf_isolated
);
criterion_main!(benches);
