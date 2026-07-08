use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::{cell::Cell, fs, path::PathBuf};
use tokio::runtime::Runtime;

use cultivator_lib::preview::{open_text_preview_impl, read_text_preview_lines_impl};

fn text_fixture_path(index: usize) -> PathBuf {
    let path = std::env::temp_dir().join(format!("cultivator_text_preview_bench_{index}.txt"));

    if path.is_file() {
        return path;
    }

    let mut text = String::with_capacity(512 * 1024);

    for line_number in 1..=8_000 {
        text.push_str(&format!(
            "{line_number:04}: timestamp=2026-07-07T12:00:00Z level=INFO event=text-preview-benchmark payload=abcdefghijklmnopqrstuvwxyz0123456789\n",
        ));
    }

    fs::write(&path, text).expect("failed to write text preview benchmark fixture");

    path
}

fn bench_text_preview(c: &mut Criterion) {
    let runtime = Runtime::new().expect("failed to create tokio runtime");
    let fixture_paths = (0..8)
        .map(|index| text_fixture_path(index).to_string_lossy().to_string())
        .collect::<Vec<_>>();
    let range_path = fixture_paths[0].clone();

    let summary = runtime
        .block_on(open_text_preview_impl(range_path.clone()))
        .expect("failed to warm text preview range benchmark");
    let middle_row = (summary.line_count / 2) as usize;
    let end_row = summary.line_count.saturating_sub(120) as usize;

    let next_fixture_index = Cell::new(0usize);
    let mut group = c.benchmark_group("text_preview");

    group.bench_function(
        BenchmarkId::new("mmap_open_summary", "8k_lines"),
        |bencher| {
            bencher.to_async(&runtime).iter(|| {
                let index = next_fixture_index.get();
                next_fixture_index.set((index + 1) % fixture_paths.len());
                let path = fixture_paths[index].clone();

                async move {
                    open_text_preview_impl(path)
                        .await
                        .expect("text preview mmap benchmark failed");
                }
            });
        },
    );

    for (label, start_row) in [("start", 0usize), ("middle", middle_row), ("end", end_row)] {
        group.bench_function(BenchmarkId::new("read_visible_range", label), |bencher| {
            bencher.to_async(&runtime).iter(|| {
                let path = range_path.clone();

                async move {
                    read_text_preview_lines_impl(path, start_row, 120)
                        .await
                        .expect("text preview range benchmark failed");
                }
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_text_preview);
criterion_main!(benches);
