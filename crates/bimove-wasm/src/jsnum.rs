//! JS 가 숫자를 글자로 바꾸는 방식을 흉내낸다.
//!
//! Bimove 화면이 띄우는 숫자와 엔진이 띄우는 숫자가 **같은 글자**여야 한다.
//! 다르면 "둘 중 어느 게 맞나" 를 아무도 못 가린다 — 특히 물량표처럼 그대로
//! 견적에 쓰이는 숫자에서는.

/// `Number.prototype.toFixed(decimals)` 와 같은 글자.
///
/// ── 왜 `{:.n}` 이 안 되나 ──
///
/// 둘 다 **double 의 정확한 값**을 보고 굽힌다. 거기까진 같다 (`8.005` 는
/// 실제로 `8.0050000000000007816` 이라 둘 다 `"8.01"` 로 간다). 갈리는 건
/// **딱 중간인 값** 하나다: Rust 는 짝수 쪽으로, JS 는 0 에서 먼 쪽으로 굽힌다.
/// `8.125` 가 Rust 에서 `"8.12"`, JS 에서 `"8.13"` 이 되는 게 그거다.
///
/// ── 중간값을 어떻게 가려내나 ──
///
/// 소수 `n` 자리에서 딱 중간이면 `v = (2k+1)/(2·10^n)` 이다. double 은
/// 이진 분수라, 분모의 `5^n` 이 분자와 지워져야 그 값이 될 수 있다. 지워지면
/// `v = j/2^(n+1)` (j 는 홀수) 꼴이 남는다. 그래서 **`v · 2^(n+1)` 이 홀수
/// 정수인지**만 보면 된다.
///
/// `n=1` 이면 `.25`·`.75` 로 끝나는 값, `n=2` 면 `.125`·`.375`·`.625`·`.875`
/// 다. 그 밖에는 `{:.n}` 이 그대로 맞다.
pub fn to_fixed(v: f64, decimals: u32) -> String {
    let n = decimals.min(17) as usize;
    if !v.is_finite() {
        return format!("{v:.n$}");
    }
    // 2^(n+1). n 을 17 로 묶어 뒀으므로 넘치지 않는다.
    let half_step = (1u64 << (n + 1)) as f64;
    let q = v * half_step;
    if q.fract() != 0.0 || q.abs() >= 9e15 || (q as i64) % 2 == 0 {
        return format!("{v:.n$}");
    }

    // 딱 중간이다. `v · 10^n` 은 정확한 반정수이고 `f64::round` 는 0 에서 먼
    // 쪽으로 굽히므로, 그게 바로 JS 가 고르는 수다.
    let scale = 10f64.powi(n as i32);
    let whole = (v * scale).round();
    if whole.abs() >= 9e15 {
        return format!("{v:.n$}");
    }
    let whole = whole as i64;
    let sign = if whole < 0 { "-" } else { "" };
    let a = whole.unsigned_abs();
    if n == 0 {
        return format!("{sign}{a}");
    }
    let d = 10u64.pow(n as u32);
    format!("{sign}{}.{:0n$}", a / d, a % d, n = n)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 아래 값들은 node 에서 `v.toFixed(n)` 을 돌려 받아온 것이다.
    #[test]
    fn 중간값을_0_에서_먼_쪽으로_굽힌다() {
        // Rust 의 `{:.2}` 는 이것들을 짝수 쪽으로 굽혀서 "8.12" 를 낸다
        assert_eq!(to_fixed(8.125, 2), "8.13");
        assert_eq!(to_fixed(8.375, 2), "8.38");
        assert_eq!(to_fixed(0.125, 2), "0.13");
        assert_eq!(to_fixed(8.25, 1), "8.3");
        assert_eq!(to_fixed(0.75, 1), "0.8");
    }

    #[test]
    fn 중간이_아니면_정확한_값을_따른다() {
        // `8.005` 는 실제로 8.0050000000000007816 — 중간보다 위다
        assert_eq!(to_fixed(8.005, 2), "8.01");
        // `1.005` 는 1.0049999999999998934 — 중간보다 아래다
        assert_eq!(to_fixed(1.005, 2), "1.00");
        assert_eq!(to_fixed(2.675, 2), "2.67");
        assert_eq!(to_fixed(8.35, 1), "8.3");
        assert_eq!(to_fixed(0.15, 1), "0.1");
        assert_eq!(to_fixed(8.45, 1), "8.4");
    }

    #[test]
    fn 자릿수를_0_으로_채운다() {
        assert_eq!(to_fixed(8.0, 2), "8.00");
        assert_eq!(to_fixed(8.1, 2), "8.10");
        assert_eq!(to_fixed(0.0, 1), "0.0");
        assert_eq!(to_fixed(1234.5, 2), "1234.50");
    }

    #[test]
    fn 소수점_없는_자리수도_된다() {
        assert_eq!(to_fixed(2.5, 0), "3");
        assert_eq!(to_fixed(3.5, 0), "4");
        assert_eq!(to_fixed(0.5, 0), "1");
        assert_eq!(to_fixed(2.4, 0), "2");
        assert_eq!(to_fixed(7.0, 0), "7");
    }

    #[test]
    fn 음수도_0_에서_먼_쪽이다() {
        assert_eq!(to_fixed(-8.125, 2), "-8.13");
        assert_eq!(to_fixed(-8.25, 1), "-8.3");
        assert_eq!(to_fixed(-2.5, 0), "-3");
        assert_eq!(to_fixed(-0.125, 2), "-0.13");
    }

    #[test]
    fn 못_쓰는_값에도_안_터진다() {
        // 그려선 안 되는 값이지만, 여기서 패닉이 나면 도면 전체가 안 열린다
        assert_eq!(to_fixed(f64::NAN, 2), "NaN");
        assert!(to_fixed(f64::INFINITY, 2).contains("inf"));
        // 아주 큰 수는 `{:.n}` 이 그대로 맡는다
        assert_eq!(to_fixed(1e20, 2), "100000000000000000000.00");
    }
}
