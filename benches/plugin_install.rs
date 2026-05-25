//! Plugin install latency benchmark (#144 v1.1).
//!
//! 量化 attune plugin pack install + verify + load 算法层 latency:
//!   1. plugin.yaml parse (serde_yaml deserialize)
//!   2. manifest schema validate (字段完整性 + 类型校验)
//!   3. SHA-256 verify (per-binary checksum)
//!   4. binary load 模拟（counts / size check，不真 spawn）
//!
//! 不在 scope:
//!   - 真 ZIP 解压（用 self-contained fixture）
//!   - 真 binary spawn（subprocess overhead 独立测）
//!   - 真签名验证（Ed25519 已由 attune-core::crypto 模块覆盖）
//!
//! 测的是 install pipeline 的纯算法层成本（YAML / hash / validate）。
//! 关键场景：plugin 安装 wizard 进度条 latency 感知（< 500 ms 用户满意）。
//!
//! 跑法：cargo bench --bench plugin_install
//! 报告：target/criterion/plugin_install/

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use sha2::{Digest, Sha256};

/// 模拟 plugin manifest YAML（典型 14-agent plugin pack）。
fn plugin_manifest_yaml() -> &'static str {
    r#"
name: law-pro
version: "1.0.0"
description: "Law domain agent pack (14 agents)"
author: "Attune Team"
license: Apache-2.0
agents:
  - id: law-pro-contract
    binary: bin/law-pro-contract
    sha256: 0000000000000000000000000000000000000000000000000000000000000001
    triggers: ["合同", "contract"]
  - id: law-pro-divorce
    binary: bin/law-pro-divorce
    sha256: 0000000000000000000000000000000000000000000000000000000000000002
    triggers: ["离婚", "财产分割"]
  - id: law-pro-traffic-accident
    binary: bin/law-pro-traffic-accident
    sha256: 0000000000000000000000000000000000000000000000000000000000000003
    triggers: ["交通事故"]
  - id: law-pro-housing-rent
    binary: bin/law-pro-housing-rent
    sha256: 0000000000000000000000000000000000000000000000000000000000000004
    triggers: ["租赁", "房屋"]
  - id: law-pro-sale-contract
    binary: bin/law-pro-sale-contract
    sha256: 0000000000000000000000000000000000000000000000000000000000000005
    triggers: ["买卖"]
  - id: law-pro-defamation
    binary: bin/law-pro-defamation
    sha256: 0000000000000000000000000000000000000000000000000000000000000006
    triggers: ["名誉", "诽谤"]
  - id: law-pro-inheritance
    binary: bin/law-pro-inheritance
    sha256: 0000000000000000000000000000000000000000000000000000000000000007
    triggers: ["继承", "遗嘱"]
  - id: extractor-contract
    binary: bin/extractor-contract
    sha256: 0000000000000000000000000000000000000000000000000000000000000008
    triggers: ["提取"]
  - id: extractor-divorce
    binary: bin/extractor-divorce
    sha256: 0000000000000000000000000000000000000000000000000000000000000009
    triggers: ["离婚财产清单"]
  - id: extractor-defamation
    binary: bin/extractor-defamation
    sha256: 000000000000000000000000000000000000000000000000000000000000000a
    triggers: ["名誉证据"]
  - id: extractor-traffic
    binary: bin/extractor-traffic
    sha256: 000000000000000000000000000000000000000000000000000000000000000b
    triggers: ["事故现场"]
  - id: extractor-housing
    binary: bin/extractor-housing
    sha256: 000000000000000000000000000000000000000000000000000000000000000c
    triggers: ["租约信息"]
  - id: general-search
    binary: bin/general-search
    sha256: 000000000000000000000000000000000000000000000000000000000000000d
    triggers: []
  - id: general-chat
    binary: bin/general-chat
    sha256: 000000000000000000000000000000000000000000000000000000000000000e
    triggers: []
"#
}

#[derive(serde::Deserialize)]
#[allow(dead_code)] // description/author/license 字段保留在 fixture 中校验 schema 完整性,
                    // 但 bench 决策路径不读;keep 是为了真实 manifest 形态对齐。
struct Manifest {
    name: String,
    version: String,
    description: String,
    author: String,
    license: String,
    agents: Vec<AgentSpec>,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)] // triggers 字段在 dispatcher 路由用到,本 bench 只测 install pipeline。
struct AgentSpec {
    id: String,
    binary: String,
    sha256: String,
    triggers: Vec<String>,
}

#[derive(Debug)]
#[allow(dead_code)] // ChecksumMismatch 是 enum 完整性需要,本 bench mock 数据不构造该 variant。
enum InstallError {
    YamlParseError,
    ValidationError(&'static str),
    ChecksumMismatch,
}

fn parse_manifest(yaml: &str) -> Result<Manifest, InstallError> {
    serde_yaml::from_str(yaml).map_err(|_| InstallError::YamlParseError)
}

fn validate_manifest(m: &Manifest) -> Result<(), InstallError> {
    if m.name.is_empty() {
        return Err(InstallError::ValidationError("empty name"));
    }
    if m.version.is_empty() {
        return Err(InstallError::ValidationError("empty version"));
    }
    if m.agents.is_empty() {
        return Err(InstallError::ValidationError("no agents"));
    }
    for a in &m.agents {
        if a.id.is_empty() || a.binary.is_empty() {
            return Err(InstallError::ValidationError("agent missing id/binary"));
        }
        // SHA-256 hex 必须 64 字符
        if a.sha256.len() != 64 {
            return Err(InstallError::ValidationError("bad sha256 length"));
        }
        if !a.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(InstallError::ValidationError("bad sha256 hex"));
        }
    }
    Ok(())
}

/// 模拟 14 个 binary（每个 1 MB，验证 SHA-256 计算 latency）
fn synthetic_binaries(count: usize, size: usize) -> Vec<Vec<u8>> {
    (0..count)
        .map(|i| {
            let mut buf = vec![0u8; size];
            buf[0] = i as u8;
            buf
        })
        .collect()
}

fn verify_binary_checksums(binaries: &[Vec<u8>]) -> Vec<[u8; 32]> {
    binaries
        .iter()
        .map(|b| {
            let mut hasher = Sha256::new();
            hasher.update(b);
            let result = hasher.finalize();
            let mut out = [0u8; 32];
            out.copy_from_slice(&result);
            out
        })
        .collect()
}

fn install_pipeline(yaml: &str, binaries: &[Vec<u8>]) -> Result<usize, InstallError> {
    let manifest = parse_manifest(yaml)?;
    validate_manifest(&manifest)?;
    let _checksums = verify_binary_checksums(binaries);
    // 若 manifest agent 数与 binary 数不一致，install fail
    if manifest.agents.len() != binaries.len() {
        return Err(InstallError::ValidationError("agent/binary count mismatch"));
    }
    Ok(manifest.agents.len())
}

fn bench_manifest_parse(c: &mut Criterion) {
    let yaml = plugin_manifest_yaml();
    let mut group = c.benchmark_group("plugin_install_yaml_parse");
    group.bench_function("parse_14_agents", |b| {
        b.iter(|| {
            let _ = parse_manifest(black_box(yaml)).unwrap();
        });
    });
    group.finish();
}

fn bench_manifest_validate(c: &mut Criterion) {
    let yaml = plugin_manifest_yaml();
    let manifest = parse_manifest(yaml).unwrap();
    let mut group = c.benchmark_group("plugin_install_validate");
    group.bench_function("validate_14_agents", |b| {
        b.iter(|| {
            let _ = validate_manifest(black_box(&manifest)).unwrap();
        });
    });
    group.finish();
}

fn bench_checksum_verify(c: &mut Criterion) {
    let mut group = c.benchmark_group("plugin_install_sha256");
    for &binary_size in &[64 * 1024usize, 1024 * 1024, 4 * 1024 * 1024] {
        let binaries = synthetic_binaries(14, binary_size);
        let total_bytes = (binaries.len() * binary_size) as u64;
        group.throughput(Throughput::Bytes(total_bytes));
        group.bench_with_input(
            BenchmarkId::from_parameter(binary_size),
            &binaries,
            |b, bins| {
                b.iter(|| {
                    let _ = verify_binary_checksums(black_box(bins));
                });
            },
        );
    }
    group.finish();
}

fn bench_install_full_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("plugin_install_full");
    let yaml = plugin_manifest_yaml();
    // 14 binary × 1 MB ≈ 14 MB plugin pack（典型大小）
    let binaries = synthetic_binaries(14, 1024 * 1024);
    group.sample_size(20);
    group.bench_function("full_install_14_agents_14MB", |b| {
        b.iter(|| {
            let _ = install_pipeline(black_box(yaml), black_box(&binaries)).unwrap();
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_manifest_parse,
    bench_manifest_validate,
    bench_checksum_verify,
    bench_install_full_pipeline
);
criterion_main!(benches);
