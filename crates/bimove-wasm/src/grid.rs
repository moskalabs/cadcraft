//! 격자.
//!
//! 한: "그리드 진행하자". 오른쪽 바의 [그리드 Show/Hide] 가 **엔진 화면에서는
//! 잠겨 있었다** — `renderPixels` 에 그런 인자가 아예 없었다. tldraw 화면에는
//! 진작 있던 것이다 (`isGridMode`).
//!
//! ── 어디에 그리나 ──
//!
//! `raster::canvas()` 로 바탕을 깐 **직후**, 밑그림과 도면보다 **앞서**.
//! 그 자리를 render 크레이트가 일부러 열어 두었다 (`render_into` 의 "anything
//! painted in between stays under the drawing"). 위에 그리면 격자가 벽을
//! 가로질러서, 그릴 때마다 도면이 더러워 보인다.
//!
//! ── 칸 간격은 줌을 따라간다 ──
//!
//! 고정 간격이면 줄여 봤을 때 선이 픽셀보다 촘촘해져 **화면이 통째로 회색**이
//! 된다 (그리고 그 선을 다 긋느라 느려진다). 그래서 1·2·5 사다리에서 화면상
//! 최소 간격을 넘는 첫 칸을 고른다 — 오토캐드와 같은 방식이다.

use cadcraft_color::Rgb;
use cadcraft_render::raster::View;
use tiny_skia::{Paint, PathBuilder, Pixmap, Stroke, Transform};

/// 격자 한 칸이 화면에서 적어도 이만큼은 돼야 한다 (**장치 픽셀**).
///
/// 호출부가 `scale` 에 dpr 을 이미 곱해 넘기므로 여기 단위도 장치 픽셀이다.
/// 8 보다 촘촘하면 사람 눈에는 격자가 아니라 **무늬**로 보인다.
const MIN_PX: f64 = 8.0;

/// 굵은 선은 몇 칸마다. 1·2·5 사다리와 맞물리게 10 이다.
const MAJOR_EVERY: i64 = 10;

/// 한 번에 그을 선의 윗한도.
///
/// `MIN_PX` 가 있어서 보통은 한참 밑이지만, 터무니없는 scale 이 들어오면
/// 수만 줄을 긋다 프레임이 통째로 멈춘다. 그럴 바엔 **안 그리는 게 낫다** —
/// 격자가 없는 건 보면 알지만, 멈춘 건 왜인지 알 수가 없다.
const MAX_LINES: i64 = 4096;

/// 바탕에서 얼마나 떨어진 색으로 그을지 (가는 선 / 굵은 선).
const MINOR_MIX: f64 = 0.07;
const MAJOR_MIX: f64 = 0.16;

/// 이 배율에서 쓸 **칸 간격** (도면 단위). 못 정하면 `None`.
///
/// 1·2·5·10·20·50… 사다리에서 화면상 `MIN_PX` 를 넘는 첫 칸을 고른다.
pub fn grid_step(scale: f64) -> Option<f64> {
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let want = MIN_PX / scale;
    if !want.is_finite() || want <= 0.0 {
        return None;
    }
    let decade = 10f64.powf(want.log10().floor());
    if !decade.is_finite() || decade <= 0.0 {
        return None;
    }
    for m in [1.0, 2.0, 5.0, 10.0] {
        let step = decade * m;
        if step >= want {
            return Some(step);
        }
    }
    None
}

/// 바탕에서 **대비 쪽으로** 섞은 색.
///
/// 밝은 바탕이면 검정 쪽으로, 어두운 바탕이면 흰 쪽으로 간다. 고정 색을
/// 쓰면 한쪽 테마에서 반드시 안 보인다 — 어두운 바탕에서 글자가 묻혔던 것과
/// 같은 사고다.
pub fn grid_color(bg: Rgb, mix: f64) -> Rgb {
    let lum = (f64::from(bg.0) + f64::from(bg.1) + f64::from(bg.2)) / 3.0;
    let target = if lum > 128.0 { 0.0 } else { 255.0 };
    let blend = |c: u8| -> u8 {
        let v = f64::from(c) + (target - f64::from(c)) * mix;
        v.round().clamp(0.0, 255.0) as u8
    };
    Rgb(blend(bg.0), blend(bg.1), blend(bg.2))
}

/// 격자를 깐다. 그릴 수 없으면 아무것도 안 하고 돌아온다.
pub fn draw(pm: &mut Pixmap, view: &View, bg: Rgb) {
    let Some(step) = grid_step(view.scale) else { return };
    let b = view.world_bounds();

    // 보이는 범위를 칸 번호로. `i64` 로 세는 이유는 **굵은 선을 나머지로**
    // 가리기 때문이다 — 좌표를 그대로 나누면 0 근처에서 부호가 흔들린다.
    let (i0, i1) = (
        (b.min.x / step).floor() as i64,
        (b.max.x / step).ceil() as i64,
    );
    let (j0, j1) = (
        (b.min.y / step).floor() as i64,
        (b.max.y / step).ceil() as i64,
    );
    if (i1 - i0) + (j1 - j0) > MAX_LINES {
        return;
    }

    let w = view.width as f32;
    let h = view.height as f32;

    // 가는 선과 굵은 선을 **각각 한 path 에 모아** 두 번만 긋는다. 선마다
    // stroke 를 부르면 호출 간접비가 선을 긋는 비용을 넘는다 (`render_into`
    // 가 같은 이유로 붓별로 모은다).
    let mut minor = PathBuilder::new();
    let mut major = PathBuilder::new();
    let put = |pb_minor: &mut PathBuilder,
                   pb_major: &mut PathBuilder,
                   n: i64,
                   a: (f32, f32),
                   z: (f32, f32)| {
        let pb = if n.rem_euclid(MAJOR_EVERY) == 0 { pb_major } else { pb_minor };
        pb.move_to(a.0, a.1);
        pb.line_to(z.0, z.1);
    };

    for i in i0..=i1 {
        let x = i as f64 * step;
        let (sx, _) = view.to_screen(cadcraft_geom::Vec2 { x, y: 0.0 });
        if !sx.is_finite() || sx < -1.0 || sx > w + 1.0 {
            continue;
        }
        // 반 픽셀 비켜 놓는다 — 정수 자리에 1px 선을 그으면 두 픽셀에 걸쳐
        // 흐려진다.
        let sx = sx.floor() + 0.5;
        put(&mut minor, &mut major, i, (sx, 0.0), (sx, h));
    }
    for j in j0..=j1 {
        let y = j as f64 * step;
        let (_, sy) = view.to_screen(cadcraft_geom::Vec2 { x: 0.0, y });
        if !sy.is_finite() || sy < -1.0 || sy > h + 1.0 {
            continue;
        }
        let sy = sy.floor() + 0.5;
        put(&mut minor, &mut major, j, (0.0, sy), (w, sy));
    }

    let stroke = Stroke { width: 1.0, ..Stroke::default() };
    for (pb, mix) in [(minor, MINOR_MIX), (major, MAJOR_MIX)] {
        let Some(path) = pb.finish() else { continue };
        let c = grid_color(bg, mix);
        let mut paint = Paint::default();
        paint.set_color_rgba8(c.0, c.1, c.2, 255);
        paint.anti_alias = false; // 가로·세로 직선이라 켤 이유가 없다
        pm.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 칸은_화면에서_너무_촘촘해지지_않는다() {
        // 어떤 배율에서도 화면 간격이 MIN_PX 밑으로 안 내려가야 한다.
        for e in -6..6 {
            let scale = 10f64.powi(e);
            let step = grid_step(scale).expect("칸을 못 골랐다");
            assert!(
                step * scale >= MIN_PX - 1e-9,
                "scale {scale}: 칸 {step} → 화면 {}px",
                step * scale
            );
        }
    }

    #[test]
    fn 칸은_1_2_5_사다리를_탄다() {
        for e in -6..6 {
            let scale = 10f64.powi(e);
            let step = grid_step(scale).unwrap();
            let m = step / 10f64.powf(step.log10().floor());
            assert!(
                (m - 1.0).abs() < 1e-6 || (m - 2.0).abs() < 1e-6 || (m - 5.0).abs() < 1e-6,
                "scale {scale}: 칸 {step} 은 1·2·5 가 아니다 (m={m})"
            );
        }
    }

    #[test]
    fn 칸은_너무_크지도_않다() {
        // 사다리 한 칸 아래는 MIN_PX 에 못 미쳐야 한다 — 쓸데없이 성기면
        // 격자가 있으나 마나다.
        for e in -4..4 {
            let scale = 10f64.powi(e);
            let step = grid_step(scale).unwrap();
            assert!(
                (step / 2.0) * scale < MIN_PX + 1e-9 || step * scale < MIN_PX * 2.5,
                "scale {scale}: 칸 {step} 이 지나치게 성기다"
            );
        }
    }

    #[test]
    fn 말이_안_되는_배율이면_안_그린다() {
        assert_eq!(grid_step(0.0), None);
        assert_eq!(grid_step(-1.0), None);
        assert_eq!(grid_step(f64::NAN), None);
        assert_eq!(grid_step(f64::INFINITY), None);
    }

    #[test]
    fn 색은_바탕_반대쪽으로_간다() {
        // 흰 바탕에서는 어두워지고, 검은 바탕에서는 밝아진다.
        let light = grid_color(Rgb(255, 255, 255), MINOR_MIX);
        assert!(light.0 < 255 && light.0 > 200, "흰 바탕: {light:?}");
        let dark = grid_color(Rgb(30, 30, 34), MINOR_MIX);
        assert!(dark.0 > 30 && dark.0 < 90, "어두운 바탕: {dark:?}");
    }

    #[test]
    fn 굵은_선이_가는_선보다_진하다() {
        for bg in [Rgb(255, 255, 255), Rgb(30, 30, 34)] {
            let minor = grid_color(bg, MINOR_MIX);
            let major = grid_color(bg, MAJOR_MIX);
            let d = |c: Rgb| (i32::from(c.0) - i32::from(bg.0)).abs();
            assert!(d(major) > d(minor), "{bg:?}: 굵은 선이 안 진하다");
        }
    }

    #[test]
    fn 격자가_픽셀에_정말_찍힌다() {
        use cadcraft_render::raster::{canvas, RasterOptions};
        let view = View {
            center: cadcraft_geom::Vec2 { x: 0.0, y: 0.0 },
            scale: 1.0,
            width: 200,
            height: 200,
        };
        let bg = Rgb(255, 255, 255);
        let ro = RasterOptions { background: bg, ..Default::default() };
        let mut pm = canvas(&view, &ro).unwrap();
        // 깔기 전에는 전부 흰색이다.
        assert!(pm.pixels().iter().all(|p| p.red() == 255 && p.green() == 255));
        draw(&mut pm, &view, bg);
        let dark = pm.pixels().iter().filter(|p| p.red() < 250).count();
        assert!(dark > 200, "격자가 안 찍혔다 (어두운 픽셀 {dark}개)");
    }

    #[test]
    fn 바탕이_어두워도_찍힌다() {
        use cadcraft_render::raster::{canvas, RasterOptions};
        let view = View {
            center: cadcraft_geom::Vec2 { x: 0.0, y: 0.0 },
            scale: 1.0,
            width: 200,
            height: 200,
        };
        let bg = Rgb(30, 30, 34);
        let ro = RasterOptions { background: bg, ..Default::default() };
        let mut pm = canvas(&view, &ro).unwrap();
        draw(&mut pm, &view, bg);
        let lit = pm.pixels().iter().filter(|p| p.red() > 35).count();
        assert!(lit > 200, "어두운 바탕에서 격자가 묻혔다 (밝은 픽셀 {lit}개)");
    }
}
