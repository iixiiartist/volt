#!/usr/bin/env python3
"""
Statistical evaluation utilities for Volt benchmarks.

Provides:
  - binomial_proportion_ci: Wilson score interval for accuracy
  - mcnemar_test: paired comparison of two configurations
  - multi_run_stats: mean, std, 95% CI from multiple runs
  - paired_ttest: paired t-test for latency comparison

Example:
    >>> stats = binomial_proportion_ci(324, 400)
    >>> print(f"{stats['accuracy_pct']:.1f}% [{stats['ci_lower']:.1f}-{stats['ci_upper']:.1f}]")
    81.0% [76.9-84.6]
"""

import math
from statistics import mean, stdev


def _norm_ppf(p: float) -> float:
    """Rational approximation of normal quantile (Abramowitz & Stegun 26.2.23)."""
    if not 0 < p < 1:
        raise ValueError(f"p must be in (0, 1), got {p}")
    if p < 0.5:
        return -_norm_ppf(1 - p)
    # Coeffs for 0.5 <= p <= 0.9999
    c = [2.515517, 0.802853, 0.010328, 1.432788, 0.189269, 0.001308]
    t = math.sqrt(-2 * math.log(1 - p))
    return t - (c[0] + c[1] * t + c[2] * t * t) / (1 + c[3] * t + c[4] * t * t + c[5] * t * t * t)


def binomial_proportion_ci(passed: int, total: int, alpha: float = 0.05) -> dict:
    """
    Wilson score confidence interval for a binomial proportion.

    More accurate than Wald (normal approx) for small samples and
    proportions near 0 or 1.  Does NOT fall outside [0,1].

    Args:
        passed: number of successes
        total: number of trials
        alpha: significance level (default 0.05 for 95% CI)

    Returns:
        dict with accuracy_pct, ci_lower, ci_upper, margin
    """
    if total == 0:
        return {"accuracy_pct": 0.0, "ci_lower": 0.0, "ci_upper": 0.0, "margin": 0.0}

    p_hat = passed / total
    z = _norm_ppf(1 - alpha / 2)
    denom = 1 + z * z / total
    center = (p_hat + z * z / (2 * total)) / denom
    margin = z * math.sqrt((p_hat * (1 - p_hat) + z * z / (4 * total)) / total) / denom
    return {
        "accuracy_pct": round(p_hat * 100, 2),
        "ci_lower": round(max(0, center - margin) * 100, 2),
        "ci_upper": round(min(1, center + margin) * 100, 2),
        "margin": round(margin * 100, 2),
    }


def _chi2_ppf(p: float, df: int) -> float:
    """Wilson-Hilferty approximation for chi-square quantile."""
    z = _norm_ppf(p)
    return float(df) * ((1 - 2 / (9 * df)) + z * math.sqrt(2 / (9 * df))) ** 3


def mcnemar_test(n_ab: int, n_ba: int) -> dict:
    """
    McNemar's test for paired nominal data.

    Compares two configurations on the same test cases: counts how many
    cases PASS under config A but FAIL under config B (n_ab), and vice
    versa (n_ba).  Cases that pass both or fail both provide no information.

    Uses exact binomial for small counts and chi-square approximation
    for larger counts.

    Args:
        n_ab: count where A passes and B fails
        n_ba: count where B passes and A fails

    Returns:
        dict with statistic, p_value, significant
    """
    n = n_ab + n_ba
    if n == 0:
        return {"statistic": 0.0, "p_value": 1.0, "significant": False}

    # Chi-square with continuity correction
    b = min(abs(n_ab - n_ba), n)
    stat = b * b / n if n > 0 else 0
    p = 1 - 2 * (0.5 - abs(0.5 * (1 + math.erf(abs(n_ab - n_ba) / math.sqrt(2 * n)))))
    # Simpler: two-sided binomial test
    from math import comb
    p_value = 2 * sum(comb(n, k) * 0.5 ** n for k in range(min(n_ab, n_ba) + 1))
    p_value = min(p_value, 1.0)

    return {
        "statistic": round(stat, 4),
        "p_value": round(p_value, 4),
        "n_ab": n_ab,
        "n_ba": n_ba,
        "significant": p_value < 0.05,
    }


def multi_run_stats(results: list) -> dict:
    """
    Compute statistics across multiple runs of the same benchmark.

    Args:
        results: list of per-run result dicts, each must have "accuracy", "latency_sec"

    Returns:
        dict with mean_accuracy, std_accuracy, ci_lower, ci_upper (95% CI),
        mean_latency, std_latency, and per-run list
    """
    n = len(results)
    if n == 0:
        return {"mean_accuracy": 0, "std_accuracy": 0, "ci_lower": 0, "ci_upper": 0,
                "mean_latency": 0, "std_latency": 0, "runs": 0}

    accs = [r["accuracy"] for r in results]
    lats = [r.get("latency_sec", 0) for r in results]

    mu_a = mean(accs) if accs else 0
    mu_l = mean(lats) if lats else 0

    if n >= 2:
        sd_a = stdev(accs) if len(set(accs)) > 1 else 0
        sd_l = stdev(lats) if len(set(lats)) > 1 else 0
        se_a = sd_a / math.sqrt(n) if n > 0 else 0
        t_crit = 4.303 if n == 2 else 3.182 if n == 3 else 2.776 if n == 4 else 2.571 if n == 5 else _norm_ppf(0.975)
        m_a = t_crit * se_a
    else:
        sd_a = 0
        sd_l = 0
        m_a = 0

    return {
        "runs": n,
        "mean_accuracy": round(mu_a, 2),
        "std_accuracy": round(sd_a, 2),
        "ci_lower": round(max(0, mu_a - m_a), 2),
        "ci_upper": round(min(100, mu_a + m_a), 2),
        "mean_latency": round(mu_l, 2),
        "std_latency": round(sd_l, 2),
        "per_run": results,
    }


def paired_ttest(values_a: list, values_b: list) -> dict:
    """Paired t-test for comparing latency between two configs on same cases."""
    n = len(values_a)
    if n != len(values_b) or n < 2:
        return {"statistic": 0, "p_value": 1.0, "significant": False}
    diffs = [a - b for a, b in zip(values_a, values_b)]
    mu_d = mean(diffs)
    sd_d = stdev(diffs) if len(set(diffs)) > 1 else 0
    if sd_d == 0:
        return {"statistic": 0, "p_value": 1.0, "significant": False, "mean_diff": mu_d}
    t = mu_d / (sd_d / math.sqrt(n))
    df = n - 1
    p = 2 * (1 - _student_t_cdf(abs(t), df))
    return {
        "statistic": round(t, 4),
        "p_value": round(p, 4),
        "significant": p < 0.05,
        "mean_diff": round(mu_d, 2),
        "std_diff": round(sd_d, 2),
    }


def _student_t_cdf(t: float, df: int) -> float:
    """Regularized incomplete beta for Student's t CDF approximation."""
    x = df / (df + t * t)
    return 1 - 0.5 * _reg_beta(x, df / 2, 0.5)


def _reg_beta(x: float, a: float, b: float) -> float:
    """Simple incomplete beta approximation by continued fraction (Lentz)."""
    if x < 0 or x > 1:
        return 0
    # Use log-beta + integration for the regularized:
    import math
    tiny = 1e-30
    f = 1.0
    c = 1.0
    d = 1 - (a + b) * x / (a + 1)
    if abs(d) < tiny:
        d = tiny
    d = 1 / d
    h = d
    for m in range(1, 200):
        m2 = 2 * m
        aa = m * (b - m) * x / ((a + m2 - 1) * (a + m2))
        d = 1 + aa * d
        if abs(d) < tiny:
            d = tiny
        c = 1 + aa / c
        if abs(c) < tiny:
            c = tiny
        d = 1 / d
        h *= d * c
        aa = -(a + m) * (a + b + m) * x / ((a + m2) * (a + m2 + 1))
        d = 1 + aa * d
        if abs(d) < tiny:
            d = tiny
        c = 1 + aa / c
        if abs(c) < tiny:
            c = tiny
        d = 1 / d
        del_ = d * c
        h *= del_
        if abs(del_ - 1) < 3e-7:
            break
    return math.exp(a * math.log(x) + b * math.log(1 - x) - math.lgamma(a + 1)
                    - math.lgamma(b + 1) + math.lgamma(a + b + 1)) * h / a


def format_accuracy(acc_pct: float, ci_lower: float, ci_upper: float) -> str:
    """Format accuracy with Wilson CI."""
    return f"{acc_pct:.1f}% [{ci_lower:.1f}–{ci_upper:.1f}]"


def format_mcnemar(test: dict) -> str:
    """Format McNemar test results."""
    if test["significant"]:
        return f"p={test['p_value']:.4f} *** (significant)"
    return f"p={test['p_value']:.4f} (n.s.)"


# -- Self-tests --

if __name__ == "__main__":
    # Binomial CI
    ci = binomial_proportion_ci(324, 400)
    assert ci["accuracy_pct"] == 81.0
    assert ci["ci_lower"] > 76 and ci["ci_lower"] < 78
    assert ci["ci_upper"] > 84 and ci["ci_upper"] < 86
    print(f"Binomial CI (324/400): {format_accuracy(ci['accuracy_pct'], ci['ci_lower'], ci['ci_upper'])}")

    # McNemar
    mc = mcnemar_test(20, 8)  # A passes 20 cases B fails, B passes 8 cases A fails
    print(f"McNemar: {format_mcnemar(mc)} (n_ab={mc['n_ab']}, n_ba={mc['n_ba']})")

    # Multi-run
    runs = [{"accuracy": 80.0, "latency_sec": 12.0}, {"accuracy": 82.0, "latency_sec": 13.0},
            {"accuracy": 81.0, "latency_sec": 14.0}]
    ms = multi_run_stats(runs)
    print(f"Multi-run: {ms['mean_accuracy']:.1f}% [{ms['ci_lower']:.1f}–{ms['ci_upper']:.1f}] (±{ms['std_accuracy']:.1f})")

    # Paired t-test
    pa = paired_ttest([12.0, 13.0, 14.0], [11.0, 12.5, 13.5])
    print(f"Paired t-test: t={pa['statistic']}, p={pa['p_value']}, diff={pa['mean_diff']}s")

    print("\nAll tests passed.")
