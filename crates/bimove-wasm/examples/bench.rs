//! 큰 도면에서 래스터 경로가 버티는지 재는 용도.
//!
//! Canvas2D 로 DisplayList 를 그리는 길은 프리미티브 10만 개에서 막혔다
//! (한 프레임 수 초). 여기선 CADCraft 자신이 쓰는 CPU 래스터라이저로 같은
//! 도면을 그려 시간을 잰다. 네이티브라 wasm 보다 빠르지만, 자릿수는 보인다.
//!
//! 쓰기: cargo run --release -p bimove-wasm --example bench -- <file.dxf> [layout]

use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: bench <file.dxf> [layout]");
    let layout = args.next().unwrap_or_default();

    let bytes = std::fs::read(&path).expect("read");
    println!("파일 {:.1}MB", bytes.len() as f64 / 1048576.0);

    let t = Instant::now();
    let drawing = cadcraft_io::read_dxf(&bytes).expect("parse");
    println!("read_dxf        {:>8.0}ms", t.elapsed().as_secs_f64() * 1000.0);

    let space = if layout.is_empty() {
        cadcraft_doc::Space::Model
    } else {
        cadcraft_doc::Space::Paper(layout.clone())
    };

    // 전체가 보이는 줌을 구한다 (화면 1600x900 가정)
    let (w, h) = (1600u32, 900u32);
    let t = Instant::now();
    let probe = cadcraft_render::build(&drawing, &space, &cadcraft_render::Options::default());
    let build_ms = t.elapsed().as_secs_f64() * 1000.0;
    let b = probe.bounds;
    println!("build (전체)     {:>8.0}ms  프리미티브 {}", build_ms, probe.prims.len());
    let fit = cadcraft_render::raster::View::fit(&b, w, h, 0.05);
    drop(probe);

    for (label, scale) in [
        ("전체보기", fit.scale),
        ("4배 줌", fit.scale * 4.0),
        ("16배 줌", fit.scale * 16.0),
        ("64배 줌", fit.scale * 64.0),
    ] {
        let opts = cadcraft_render::Options {
            tolerance: 0.5 / scale.max(1e-12),
            ..Default::default()
        };
        let t = Instant::now();
        let dl = cadcraft_render::build(&drawing, &space, &opts);
        let bms = t.elapsed().as_secs_f64() * 1000.0;

        let view = cadcraft_render::raster::View { center: fit.center, scale, width: w, height: h };
        let ro = cadcraft_render::raster::RasterOptions {
            background: cadcraft_color::Rgb(255, 255, 255),
            ..Default::default()
        };
        let t = Instant::now();
        let pm = cadcraft_render::raster::render(&dl, &view, &ro);
        let rms = t.elapsed().as_secs_f64() * 1000.0;
        println!(
            "{label:<10} build {bms:>7.0}ms  raster {rms:>7.0}ms  합 {:>7.0}ms  프리미티브 {}  {}",
            bms + rms,
            dl.prims.len(),
            if pm.is_some() { "" } else { "(래스터 실패)" }
        );
    }

    // 눈으로 볼 수 있게 한 장 남긴다
    let opts = cadcraft_render::Options { tolerance: 0.5 / fit.scale, ..Default::default() };
    let dl = cadcraft_render::build(&drawing, &space, &opts);
    let ro = cadcraft_render::raster::RasterOptions {
        background: cadcraft_color::Rgb(255, 255, 255),
        ..Default::default()
    };
    if let Some(png) = cadcraft_render::raster::render_png(&dl, &fit, &ro) {
        let out = format!("{path}.{}.png", if layout.is_empty() { "model" } else { &layout });
        std::fs::write(&out, png).expect("write png");
        println!("저장: {out}");
    }
}
