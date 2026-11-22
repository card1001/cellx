//! CellX: BESSELJ and BESSELY as Excel computes them. Excel uses the rational
//! and asymptotic approximations of Numerical Recipes (bessj0, bessj1, bessy0,
//! bessy1) with recurrences for higher orders, about 8 significant digits;
//! BESSELJ(0,0) is 1.00000000283141 there, not 1.

fn j0(x: f64) -> f64 {
    let ax = x.abs();
    if ax < 8.0 {
        let y = x * x;
        let ans1 = 57568490574.0
            + y * (-13362590354.0
                + y * (651619640.7 + y * (-11214424.18 + y * (77392.33017 + y * (-184.9052456)))));
        let ans2 = 57568490411.0
            + y * (1029532985.0 + y * (9494680.718 + y * (59272.64853 + y * (267.8532712 + y))));
        ans1 / ans2
    } else {
        let z = 8.0 / ax;
        let y = z * z;
        let xx = ax - 0.785398164;
        let ans1 = 1.0
            + y * (-0.1098628627e-2 + y * (0.2734510407e-4 + y * (-0.2073370639e-5 + y * 0.2093887211e-6)));
        let ans2 = -0.1562499995e-1
            + y * (0.1430488765e-3 + y * (-0.6911147651e-5 + y * (0.7621095161e-6 - y * 0.934935152e-7)));
        (0.636619772 / ax).sqrt() * (xx.cos() * ans1 - z * xx.sin() * ans2)
    }
}

fn j1(x: f64) -> f64 {
    let ax = x.abs();
    if ax < 8.0 {
        let y = x * x;
        let ans1 = x
            * (72362614232.0
                + y * (-7895059235.0
                    + y * (242396853.1 + y * (-2972611.439 + y * (15704.48260 + y * (-30.16036606))))));
        let ans2 = 144725228442.0
            + y * (2300535178.0 + y * (18583304.74 + y * (99447.43394 + y * (376.9991397 + y))));
        ans1 / ans2
    } else {
        let z = 8.0 / ax;
        let y = z * z;
        let xx = ax - 2.356194491;
        let ans1 = 1.0
            + y * (0.183105e-2 + y * (-0.3516396496e-4 + y * (0.2457520174e-5 + y * (-0.240337019e-6))));
        let ans2 = 0.04687499995
            + y * (-0.2002690873e-3 + y * (0.8449199096e-5 + y * (-0.88228987e-6 + y * 0.105787412e-6)));
        let ans = (0.636619772 / ax).sqrt() * (xx.cos() * ans1 - z * xx.sin() * ans2);
        if x < 0.0 {
            -ans
        } else {
            ans
        }
    }
}

fn y0(x: f64) -> f64 {
    if x < 8.0 {
        let y = x * x;
        let ans1 = -2957821389.0
            + y * (7062834065.0
                + y * (-512359803.6 + y * (10879881.29 + y * (-86327.92757 + y * 228.4622733))));
        let ans2 = 40076544269.0
            + y * (745249964.8 + y * (7189466.438 + y * (47447.26470 + y * (226.1030244 + y))));
        ans1 / ans2 + 0.636619772 * j0(x) * x.ln()
    } else {
        let z = 8.0 / x;
        let y = z * z;
        let xx = x - 0.785398164;
        let ans1 = 1.0
            + y * (-0.1098628627e-2 + y * (0.2734510407e-4 + y * (-0.2073370639e-5 + y * 0.2093887211e-6)));
        let ans2 = -0.1562499995e-1
            + y * (0.1430488765e-3 + y * (-0.6911147651e-5 + y * (0.7621095161e-6 + y * (-0.934945152e-7))));
        (0.636619772 / x).sqrt() * (xx.sin() * ans1 + z * xx.cos() * ans2)
    }
}

fn y1(x: f64) -> f64 {
    if x < 8.0 {
        let y = x * x;
        let ans1 = x
            * (-0.4900604943e13
                + y * (0.1275274390e13
                    + y * (-0.5153438139e11 + y * (0.7349264551e9 + y * (-0.4237922726e7 + y * 0.8511937935e4)))));
        let ans2 = 0.2499580570e14
            + y * (0.4244419664e12
                + y * (0.3733650367e10 + y * (0.2245904002e8 + y * (0.1020426050e6 + y * (0.3549632885e3 + y)))));
        ans1 / ans2 + 0.636619772 * (j1(x) * x.ln() - 1.0 / x)
    } else {
        let z = 8.0 / x;
        let y = z * z;
        let xx = x - 2.356194491;
        let ans1 = 1.0
            + y * (0.183105e-2 + y * (-0.3516396496e-4 + y * (0.2457520174e-5 + y * (-0.240337019e-6))));
        let ans2 = 0.04687499995
            + y * (-0.2002690873e-3 + y * (0.8449199096e-5 + y * (-0.88228987e-6 + y * 0.105787412e-6)));
        (0.636619772 / x).sqrt() * (xx.sin() * ans1 + z * xx.cos() * ans2)
    }
}

/// J_n(x), n >= 0.
pub(crate) fn besselj(n: i32, x: f64) -> f64 {
    match n {
        0 => return j0(x),
        1 => return j1(x),
        _ => {}
    }
    let ax = x.abs();
    if ax == 0.0 {
        return 0.0;
    }
    let tox = 2.0 / ax;
    let ans = if ax > n as f64 {
        // Upward recurrence from J0 and J1.
        let mut bjm = j0(ax);
        let mut bj = j1(ax);
        for j in 1..n {
            let bjp = j as f64 * tox * bj - bjm;
            bjm = bj;
            bj = bjp;
        }
        bj
    } else {
        // Miller's downward recurrence.
        const ACC: f64 = 40.0;
        const BIGNO: f64 = 1.0e10;
        const BIGNI: f64 = 1.0e-10;
        let m = 2 * ((n + (ACC * n as f64).sqrt() as i32) / 2);
        let mut jsum = false;
        let (mut bjp, mut ans, mut sum) = (0.0, 0.0, 0.0);
        let mut bj = 1.0;
        let mut j = m;
        while j > 0 {
            let bjm = j as f64 * tox * bj - bjp;
            bjp = bj;
            bj = bjm;
            if bj.abs() > BIGNO {
                bj *= BIGNI;
                bjp *= BIGNI;
                ans *= BIGNI;
                sum *= BIGNI;
            }
            if jsum {
                sum += bj;
            }
            jsum = !jsum;
            if j == n {
                ans = bjp;
            }
            j -= 1;
        }
        sum = 2.0 * sum - bj;
        ans / sum
    };
    if x < 0.0 && n % 2 == 1 {
        -ans
    } else {
        ans
    }
}

/// Y_n(x), n >= 0, x > 0 (NaN otherwise).
pub(crate) fn bessely(n: i32, x: f64) -> f64 {
    if x <= 0.0 {
        return f64::NAN;
    }
    match n {
        0 => return y0(x),
        1 => return y1(x),
        _ => {}
    }
    let tox = 2.0 / x;
    let mut by = y1(x);
    let mut bym = y0(x);
    for j in 1..n {
        let byp = j as f64 * tox * by - bym;
        bym = by;
        by = byp;
    }
    by
}
