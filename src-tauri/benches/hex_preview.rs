use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::{fs, hint::black_box, path::Path};
use tempfile::TempDir;

use cultivator_lib::preview::hex;

const HEX_PAGE_BYTES: u64 = 16 * 1024;

fn binary_fixture(directory: &Path, size: usize) -> String {
    // Mixed printable and non-printable bytes exercise both branches of the ASCII gutter.
    let bytes = (0..size)
        .map(|index| (index.wrapping_mul(31) ^ (index >> 7)) as u8)
        .collect::<Vec<_>>();
    let path = directory.join("hex_preview.bin");
    fs::write(&path, bytes).expect("failed to write hex preview fixture");
    path.to_string_lossy().to_string()
}

fn bench_hex_preview(c: &mut Criterion) {
    // A single-threaded executor keeps async scheduling inside the measured thread.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to create tokio runtime");
    let fixtures = TempDir::new().expect("failed to create benchmark fixture directory");
    let path = binary_fixture(fixtures.path(), 1024 * 1024);

    let summary = runtime
        .block_on(hex::open(path.clone()))
        .expect("failed to open hex preview fixture");
    let last_page = summary.page_count.saturating_sub(1);

    let mut group = c.benchmark_group("hex_preview");
    group.bench_function("search_preview_512_bytes", |b| {
        b.iter(|| {
            runtime
                .block_on(hex::preview(black_box(path.clone())))
                .expect("hex search preview failed")
        })
    });

    group.throughput(Throughput::Bytes(HEX_PAGE_BYTES));
    for (label, page_index) in [("first", 0), ("middle", last_page / 2), ("last", last_page)] {
        group.bench_function(BenchmarkId::new("read_page", label), |b| {
            b.iter(|| {
                runtime
                    .block_on(hex::read_page(
                        black_box(path.clone()),
                        black_box(page_index),
                    ))
                    .expect("hex page read failed")
            })
        });
    }
    group.finish();
}

criterion_group!(benches, bench_hex_preview);
criterion_main!(benches);
