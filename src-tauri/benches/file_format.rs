use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use cultivator_lib::preview::{strings::sqlite_file_format_preview, video};
use std::hint::black_box;

fn sqlite_header() -> [u8; 100] {
    let mut header = [0; 100];
    header[..16].copy_from_slice(b"SQLite format 3\0");
    header[16..18].copy_from_slice(&4096_u16.to_be_bytes());
    header[18] = 1;
    header[19] = 1;
    header[21] = 64;
    header[22] = 32;
    header[23] = 32;
    header[28..32].copy_from_slice(&32_768_u32.to_be_bytes());
    header[40..44].copy_from_slice(&4_u32.to_be_bytes());
    header[56..60].copy_from_slice(&1_u32.to_be_bytes());
    header
}

fn bench_file_format(c: &mut Criterion) {
    let header = sqlite_header();
    let mut group = c.benchmark_group("file_format");
    group.throughput(Throughput::Elements(1));
    group.bench_function("sqlite_header", |b| {
        b.iter(|| sqlite_file_format_preview(black_box(&header)))
    });

    let preview = sqlite_file_format_preview(&header);
    group.bench_function("sqlite_preview_serialization", |b| {
        b.iter(|| serde_json::to_vec(black_box(&preview)).expect("failed to serialize preview"))
    });

    for brand in ["isom", "mp42", "qt  "] {
        let mut header = [0; 24];
        header[..4].copy_from_slice(&24_u32.to_be_bytes());
        header[4..8].copy_from_slice(b"ftyp");
        header[8..12].copy_from_slice(brand.as_bytes());
        group.bench_function(BenchmarkId::new("video_header", brand.trim()), |b| {
            b.iter(|| video::preview(black_box("evidence/clip.mp4"), black_box(&header)))
        });
    }
    group.finish();
}

criterion_group!(benches, bench_file_format);
criterion_main!(benches);
