//! Microbench for waterfall UI CPU path (compose + colormap + pixel scroll).
//! Run: cargo test --release --test waterfall_render_bench -- --nocapture

use std::time::Instant;

use hfsdr::{compose_panadapter_row, stretch_row_to_width, SpectrumViewMapping};

type Pixel = [u8; 4];

const BLACK: Pixel = [0, 0, 0, 255];

const WATERFALL_ROWS: usize = 360;

fn db_to_colour(db: f32, ref_db: f32, range_db: f32) -> Pixel {
    let floor = ref_db - range_db;
    let t = ((db - floor) / range_db).clamp(0.0, 1.0);
    let t = t.powf(1.15);
    let r = (t * 255.0) as u8;
    let g = (t * 200.0) as u8;
    let b = ((1.0 - t) * 180.0 + t * 255.0) as u8;
    [r, g, b, 255]
}

fn write_row_pixels(pixels: &mut [Pixel], y: usize, width: usize, db_row: &[f32], ref_db: f32, range_db: f32) {
    let base = y * width;
    for (x, &db) in db_row.iter().enumerate().take(width) {
        pixels[base + x] = db_to_colour(db, ref_db, range_db);
    }
}

fn row_db_for_viewport(
    src_row: &[f32],
    view: &SpectrumViewMapping,
    width: usize,
) -> Vec<f32> {
    let row = compose_panadapter_row(
        src_row,
        view.row_rate_hz,
        view.view_span_hz,
        view.data_span_hz,
        view.compose_pan_offset_hz,
        view.allow_band_padding,
    );
    let stretched = stretch_row_to_width(&row, width);
    stretched
}

fn bench_append_row(plot_width: usize, fft_size: usize, n_rows: usize) -> f64 {
    let view = view_12k();
    let src_row: Vec<f32> = (0..fft_size).map(|i| -90.0 + (i % 40) as f32).collect();
    let mut pixels = vec![BLACK; plot_width * WATERFALL_ROWS];
    let ref_db = -20.0f32;
    let range_db = 80.0f32;

    let t0 = Instant::now();
    for _ in 0..n_rows {
        let stride = plot_width;
        for y in (0..WATERFALL_ROWS - 1).rev() {
            let src = y * stride;
            pixels.copy_within(src..src + stride, (y + 1) * stride);
        }
        let row_db = row_db_for_viewport(&src_row, &view, plot_width);
        write_row_pixels(&mut pixels, 0, plot_width, &row_db, ref_db, range_db);
    }
    t0.elapsed().as_secs_f64() / n_rows as f64 * 1000.0
}

fn view_12k() -> SpectrumViewMapping {
    SpectrumViewMapping {
        row_rate_hz: 12_000.0,
        view_span_hz: 12_000.0,
        data_span_hz: 12_000.0,
        pan_offset_hz: 0.0,
        compose_pan_offset_hz: 0.0,
        allow_band_padding: false,
    }
}

fn bench_full_rebuild(plot_width: usize, fft_size: usize) -> f64 {
    let view = view_12k();
    let src_row: Vec<f32> = (0..fft_size).map(|i| -90.0 + (i % 40) as f32).collect();
    let ref_db = -20.0f32;
    let range_db = 80.0f32;

    let t0 = Instant::now();
    let mut pixels = vec![BLACK; plot_width * WATERFALL_ROWS];
    for y in 0..WATERFALL_ROWS {
        let row_db = row_db_for_viewport(&src_row, &view, plot_width);
        write_row_pixels(&mut pixels, y, plot_width, &row_db, ref_db, range_db);
    }
    t0.elapsed().as_secs_f64() * 1000.0
}

#[test]
fn waterfall_ui_render_cost() {
    let plot_widths = [1200usize, 1580, 1920];
    let fft_sizes = [2048usize, 4096, 8192];
    eprintln!("\n=== waterfall UI CPU bench (no GPU upload) ===");
    for &fft in &fft_sizes {
        for &w in &plot_widths {
            let append_ms = bench_append_row(w, fft, 500);
            let full_ms = bench_full_rebuild(w, fft);
            eprintln!(
                "fft={fft:5} plot_w={w:4}  append_row={append_ms:.2} ms  full_rebuild={full_ms:.1} ms"
            );
        }
    }
    eprintln!("Note: GPU upload clones ~{:.1} MB per append at 1580×360", (1580 * 360 * 4) as f64 / 1e6);
}
