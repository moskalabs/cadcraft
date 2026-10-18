//! Bimove 도면을 엔진으로 그릴 때 한 프레임이 얼마나 드는지 잰다.
//!
//! 한이 "줌 인/아웃 할 때 오른쪽에 텍스트 잔상이 잠깐 남는다" 고 했다. 캔버스는
//! 매 프레임 배경색으로 덮고 통째로 다시 찍으므로, **한 프레임 안에서 잔상이
//! 생길 수는 없다.** 남은 가능성은 그 프레임이 너무 오래 걸려서 브라우저가
//! 직전 그림을 계속 보여준 것 하나다. 그래서 여기서 재는 건 프레임 시간이다.
//!
//! 도면은 **여기서 만든다.** 고객 도면을 레포에 넣지 않으려는 것이고, 씨를
//! 고정했으니 누가 언제 돌려도 같은 숫자가 나온다. 모양은 진짜 도면을 닮게
//! 잡았다 — Bimove 는 폴리라인을 `M a L b M b L c …` 로 쪼개 저장하므로,
//! 무작위로 흩어진 선분으로 재면 이어 붙이기의 값어치가 안 보인다.
//!
//! ```text
//! cargo run --release -p bimove-wasm --example bimove_bench
//! ```

use std::time::Instant;

/// 씨 고정 난수. 돌릴 때마다 같은 도면이 나와야 숫자를 견줄 수 있다.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
}

/// 폴리라인이 M/L 짝으로 쪼개져 들어간, 진짜 도면을 닮은 Bimove 문서.
fn make_doc(groups: usize, polys: usize, pts: usize) -> String {
    let mut r = Lcg(0x5EED);
    let mut shapes = Vec::with_capacity(groups);
    for g in 0..groups {
        let (ox, oy) = (r.next() * 4000.0, r.next() * 3000.0);
        let mut path = String::new();
        for _ in 0..polys {
            let (mut x, mut y) = (r.next() * 200.0, r.next() * 200.0);
            for _ in 0..pts - 1 {
                // 들여올 때처럼 소수 첫째 자리로 반올림한다 — 그래야 이어
                // 붙이기가 실제와 같은 조건에서 돈다.
                let nx = (x + (r.next() - 0.5) * 80.0).round() / 10.0;
                let ny = (y + (r.next() - 0.5) * 80.0).round() / 10.0;
                path.push_str(&format!("M{x:.1},{y:.1}L{nx:.1},{ny:.1}"));
                x = nx;
                y = ny;
            }
        }
        let texts = if g % 20 == 0 {
            r#""[{\"x\":5,\"y\":5,\"t\":\"거실 마감\",\"h\":8,\"ap\":1}]""#
        } else {
            r#""""#
        };
        shapes.push(format!(
            r#"{{"id":"shape:{g}","type":"dxfgroup","x":{ox:.1},"y":{oy:.1},"rotation":0,"index":"a1","parentId":"page:main","props":{{"pathData":"{path}","textsJson":{texts}}},"meta":{{"dxfLayer":"L{}"}}}}"#,
            g % 12
        ));
    }
    format!(
        r#"{{"version":1,"pages":[{{"id":"page:main","name":"모형","index":"a1"}}],"shapes":[{}],"records":{{}},"skipped":0,"source":{{"from":"tldraw","at":0,"sourceHash":"x","shapeCount":{groups}}}}}"#,
        shapes.join(",")
    )
}

fn main() {
    let json = match std::env::args().nth(1) {
        Some(p) => std::fs::read_to_string(&p).expect("문서를 못 읽었다"),
        None => make_doc(400, 40, 38),
    };
    println!("문서 {:.1}MB", json.len() as f64 / 1048576.0);

    let t = Instant::now();
    let load = bimove_wasm::bimove::drawing_from_bimove(&json, "").expect("변환 실패");
    println!(
        "변환 {:>6.0}ms · 선분 {} → 엔티티 {} ({:.1}배로 묶임) / 글자 {}",
        t.elapsed().as_secs_f64() * 1000.0,
        load.segment_count,
        load.line_count,
        load.segment_count as f64 / load.line_count.max(1) as f64,
        load.text_count
    );

    let mut doc = bimove_wasm::Doc::from_bimove(&json, "").expect("열기 실패");
    let (w, h) = (1874u32, 1000u32);
    let (cx, cy) = (2000.0, -1500.0);

    // **만드는 비용**과 **찍는 비용**을 가른다. 어느 쪽이 큰지 모르고 고치면
    // 엉뚱한 데를 고친다.
    for sc in [0.2_f64, 2.0, 50.0] {
        let t = Instant::now();
        let f = doc.build_for_bench("", 0.5 / sc);
        let build = t.elapsed().as_secs_f64() * 1000.0;
        let prims = f.prims.len() / 6;
        doc.raster_for_bench("", cx, cy, sc, w, h, 0xFFFFFF, false);
        let t = Instant::now();
        doc.raster_for_bench("", cx, cy, sc, w, h, 0xFFFFFF, false);
        let draw = t.elapsed().as_secs_f64() * 1000.0;
        println!("  scale {sc:>7.2} · 만들기 {build:>7.1}ms (prim {prims}) · 찍기 {draw:>7.1}ms");
    }

    // 조작 중에 해상도를 반으로 떨어뜨리면 픽셀이 1/4 이 된다 — 흔한 수다.
    //
    // **여기선 별로 안 벌린다.** 재보니 242→186ms, 54→33ms 다. 비용이 픽셀이
    // 아니라 **선분 개수**에 붙어 있기 때문이다 (59만 선분을 해상도와 무관하게
    // 매번 화면 좌표로 옮기고 stroke 한다). 그래서 안 넣었다 — 이 줄은 그
    // 측정을 남겨두려고 있다. 같은 이유로 "path 를 묶어서 한 번에 stroke" 도
    // 기대만큼 안 벌 것이다.
    //
    // 진짜 레버는 **선분을 덜 그리는 것** 하나다 (전체 보기에서 1픽셀도 안 되는
    // 디테일을 간략화한 사본을 따로 들고 갈아끼우기).
    for (lbl, dw, dh, mul) in [("전체해상도", w, h, 1.0_f64), ("절반해상도", w / 2, h / 2, 0.5)] {
        for sc in [0.2_f64, 2.0] {
            doc.raster_for_bench("", cx, cy, sc * mul, dw, dh, 0xFFFFFF, true);
            let t = Instant::now();
            doc.raster_for_bench("", cx, cy, sc * mul, dw, dh, 0xFFFFFF, true);
            println!(
                "  {lbl} scale {sc:>5.2} → {:>6.1}ms",
                t.elapsed().as_secs_f64() * 1000.0
            );
        }
    }

    for draft in [false, true] {
        sweep(&mut doc, w, h, cx, cy, draft);
    }
}

/// 휠을 끝까지 굴리는 동안의 프레임 시간. `draft` 면 목록을 다시 안 만든다.
fn sweep(doc: &mut bimove_wasm::Doc, w: u32, h: u32, cx: f64, cy: f64, draft: bool) {
    let mut scale = 0.2_f64;
    let mut worst = 0.0_f64;
    let mut worst_at = 0.0_f64;
    let mut total = 0.0_f64;
    let n = 40;
    for i in 0..n {
        let t = Instant::now();
        let r = doc.raster_for_bench("", cx, cy, scale, w, h, 0xFFFFFF, draft);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        total += ms;
        if ms > worst {
            worst = ms;
            worst_at = scale;
        }
        if r.is_none() {
            println!("  !! scale {scale:.4} 에서 래스터 실패 — 이 프레임은 안 그려진다");
        }
        if i % 10 == 0 {
            println!("    scale {scale:>9.4} → {ms:>6.1}ms");
        }
        scale *= 1.25;
    }
    println!(
        "{} 평균 {:.1}ms · 최악 {:.1}ms (scale {:.4})",
        if draft { "draft " } else { "정식  " },
        total / f64::from(n),
        worst,
        worst_at
    );
}
