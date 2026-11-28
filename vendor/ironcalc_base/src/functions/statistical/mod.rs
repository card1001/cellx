mod beta;
mod binom;
mod chisq;
mod correl;
mod count_and_average;
mod covariance;
mod devsq;
mod exponential;
mod fisher;
mod forecast;
mod frequency;
mod gamma;
mod gauss;
mod geomean;
mod hypegeom;
mod if_ifs;
mod linest;
mod log_normal;
mod mode_functions;
mod normal;
mod pearson;
mod percentile;
mod permut;
mod phi;
mod poisson;
mod prob;
mod quartile;
mod rank_eq_avg;
mod standard_dev;
mod standardize;
mod t_dist;
mod trimmean;
mod variance;
mod weibull;
mod z_test;

/// statrs's inverse distribution functions can panic on extreme inputs (an
/// `unwrap` inside its inverse regularized beta function). A formula must
/// never stop the program, so a panic becomes NaN, which the callers report
/// as #NUM!.
pub(crate) fn guarded_inverse(f: impl FnOnce() -> f64) -> f64 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(f64::NAN)
}

/// The inverse of an increasing CDF by bracketing and bisection, for when
/// the closed-form inverse fails.
pub(crate) fn invert_cdf(cdf: impl Fn(f64) -> f64, p: f64) -> f64 {
    let (mut lo, mut hi) = (-1.0_f64, 1.0_f64);
    while cdf(lo) > p {
        lo *= 2.0;
        if !lo.is_finite() {
            return f64::NAN;
        }
    }
    while cdf(hi) < p {
        hi *= 2.0;
        if !hi.is_finite() {
            return f64::NAN;
        }
    }
    for _ in 0..400 {
        let mid = lo + (hi - lo) / 2.0;
        if mid <= lo || mid >= hi || (hi - lo) <= 1e-15 * mid.abs().max(1e-300) {
            return mid;
        }
        if cdf(mid) < p {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo + (hi - lo) / 2.0
}

/// The lower-tail inverse of Student's t distribution with `df` degrees of
/// freedom. One and two degrees of freedom have exact formulas, which stay
/// accurate far into the tails (T.INV(1E-300, 1) is -3.18E+299 in Excel).
pub(crate) fn t_inverse(dist: &statrs::distribution::StudentsT, df: f64, p: f64) -> f64 {
    use statrs::distribution::ContinuousCDF;
    use std::f64::consts::PI;
    if df == 1.0 {
        return if p == 0.5 {
            0.0
        } else if p < 0.5 {
            -1.0 / (PI * p).tan()
        } else {
            1.0 / (PI * (1.0 - p)).tan()
        };
    }
    if df == 2.0 {
        return (2.0 * p - 1.0) / (2.0 * p * (1.0 - p)).sqrt();
    }
    let x = guarded_inverse(|| dist.inverse_cdf(p));
    if x.is_finite() {
        return x;
    }
    invert_cdf(|x| ContinuousCDF::cdf(dist, x), p)
}
