//! Encrypt overhead benchmark (#122 v1.1).
//!
//! 测量 attune 字段级 AES-256-GCM 加密的 overhead，相对未加密 baseline。
//! 4 维度：
//!   1. Argon2id KDF 一次性成本 (vault setup/unlock)
//!   2. encrypt round-trip (per item save)
//!   3. decrypt round-trip (per item get)
//!   4. encrypt+decrypt 不同 payload 大小 (1 KB / 10 KB / 100 KB)
//!
//! 输出：
//!   - 每个操作的 ns / ops
//!   - HTML 报告：rust/target/criterion/encrypt_overhead/
//!
//! 用于 marketing claim：「字段级加密 overhead < 5%」需要 < 50µs per save (假设 1ms baseline)。
//! 实测 < 50 µs 即视为可忽略 overhead（典型 DB INSERT 1-5 ms，加密占比 < 5%）。

use attune_core::crypto;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

fn bench_argon2_kdf(c: &mut Criterion) {
    let mut group = c.benchmark_group("argon2_kdf");
    // KDF 是 vault unlock 单次成本（不在热路径），target ≥ 200 ms 表示足够抗暴力，
    // 测一次即可，不需要多 sample。
    group.sample_size(10);
    group.bench_function("derive_master_key_64MB_t3_p4", |b| {
        let salt = crypto::generate_salt();
        let password = b"correct horse battery staple";
        let device_secret = [0u8; 32];
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

fn bench_encrypt_payload_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("aes_gcm_encrypt");
    let key = crypto::Key32::generate();
    for &size in &[1024usize, 10_240, 102_400] {
        let payload = vec![0x42u8; size];
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &payload, |b, p| {
            b.iter(|| {
                let _ct = crypto::encrypt(black_box(&key), black_box(p)).unwrap();
            });
        });
    }
    group.finish();
}

fn bench_decrypt_payload_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("aes_gcm_decrypt");
    let key = crypto::Key32::generate();
    for &size in &[1024usize, 10_240, 102_400] {
        let payload = vec![0x42u8; size];
        let ct = crypto::encrypt(&key, &payload).unwrap();
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &ct, |b, ct| {
            b.iter(|| {
                let _pt = crypto::decrypt(black_box(&key), black_box(ct)).unwrap();
            });
        });
    }
    group.finish();
}

/// 端到端 round-trip（典型 item save → load 路径）。
fn bench_round_trip(c: &mut Criterion) {
    let mut group = c.benchmark_group("encrypt_decrypt_roundtrip");
    let key = crypto::Key32::generate();
    // 4 KB 是典型 chunk 大小（512 chars × ~8 bytes UTF-8）
    let payload = vec![0xABu8; 4096];
    group.bench_function("4KB_payload", |b| {
        b.iter(|| {
            let ct = crypto::encrypt(black_box(&key), black_box(&payload)).unwrap();
            let _pt = crypto::decrypt(black_box(&key), black_box(&ct)).unwrap();
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_argon2_kdf,
    bench_encrypt_payload_sizes,
    bench_decrypt_payload_sizes,
    bench_round_trip
);
criterion_main!(benches);
