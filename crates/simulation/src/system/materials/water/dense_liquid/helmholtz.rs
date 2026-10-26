//! IAPWS R6-95(2018), eqs. 4–6 and tables 2–3. Only the residual Helmholtz
//! value and first density derivative are required for pressure; no entropy,
//! heat capacity or phase-equilibrium potential is exposed.

const CRITICAL_DENSITY: f64 = 322.0;
const CRITICAL_TEMPERATURE: f64 = 647.096;
const GAS_CONSTANT: f64 = 461.51805;

// Table 2, rows 1–51: n, d, t, c. c=0 denotes the undamped first seven terms.
const TERMS: [(f64, i32, f64, i32); 51] = [
    (0.12533547935523e-1, 1, -0.5, 0),
    (0.78957634722828e1, 1, 0.875, 0),
    (-0.87803203303561e1, 1, 1.0, 0),
    (0.31802509345418, 2, 0.5, 0),
    (-0.26145533859358, 2, 0.75, 0),
    (-0.78199751687981e-2, 3, 0.375, 0),
    (0.88089493102134e-2, 4, 1.0, 0),
    (-0.66856572307965, 1, 4.0, 1),
    (0.20433810950965, 1, 6.0, 1),
    (-0.66212605039687e-4, 1, 12.0, 1),
    (-0.19232721156002, 2, 1.0, 1),
    (-0.25709043003438, 2, 5.0, 1),
    (0.16074868486251, 3, 4.0, 1),
    (-0.40092828925807e-1, 4, 2.0, 1),
    (0.39343422603254e-6, 4, 13.0, 1),
    (-0.75941377088144e-5, 5, 9.0, 1),
    (0.56250979351888e-3, 7, 3.0, 1),
    (-0.15608652257135e-4, 9, 4.0, 1),
    (0.11537996422951e-8, 10, 11.0, 1),
    (0.36582165144204e-6, 11, 4.0, 1),
    (-0.13251180074668e-11, 13, 13.0, 1),
    (-0.62639586912454e-9, 15, 1.0, 1),
    (-0.10793600908932, 1, 7.0, 2),
    (0.17611491008752e-1, 2, 1.0, 2),
    (0.22132295167546, 2, 9.0, 2),
    (-0.40247669763528, 2, 10.0, 2),
    (0.58083399985759, 3, 10.0, 2),
    (0.49969146990806e-2, 4, 3.0, 2),
    (-0.31358700712549e-1, 4, 7.0, 2),
    (-0.74315929710341, 4, 10.0, 2),
    (0.47807329915480, 5, 10.0, 2),
    (0.20527940895948e-1, 6, 6.0, 2),
    (-0.13636435110343, 6, 10.0, 2),
    (0.14180634400617e-1, 7, 10.0, 2),
    (0.83326504880713e-2, 9, 1.0, 2),
    (-0.29052336009585e-1, 9, 2.0, 2),
    (0.38615085574206e-1, 9, 3.0, 2),
    (-0.20393486513704e-1, 9, 4.0, 2),
    (-0.16554050063734e-2, 9, 8.0, 2),
    (0.19955571979541e-2, 10, 6.0, 2),
    (0.15870308324157e-3, 10, 9.0, 2),
    (-0.16388568342530e-4, 12, 8.0, 2),
    (0.43613615723811e-1, 3, 16.0, 3),
    (0.34994005463765e-1, 4, 22.0, 3),
    (-0.76788197844621e-1, 4, 23.0, 3),
    (0.22446277332006e-1, 5, 23.0, 3),
    (-0.62689710414685e-4, 14, 10.0, 4),
    (-0.55711118565645e-9, 3, 50.0, 6),
    (-0.19905718354408, 6, 44.0, 6),
    (0.31777497330738, 6, 46.0, 6),
    (-0.11841182425981, 6, 50.0, 6),
];

pub(super) fn pressure_pa(density: f64, temperature: f64) -> f64 {
    let delta = density / CRITICAL_DENSITY;
    let tau = CRITICAL_TEMPERATURE / temperature;
    let (_, derivative) = residual(delta, tau);
    density * GAS_CONSTANT * temperature * (1.0 + delta * derivative)
}

/// Residual Helmholtz energy and its analytic partial derivative w.r.t. delta.
pub(super) fn residual(delta: f64, tau: f64) -> (f64, f64) {
    let mut value = 0.0;
    let mut derivative = 0.0;
    for (n, d, t, c) in TERMS {
        let damping = if c == 0 { 0.0 } else { delta.powi(c) };
        let term = n * delta.powi(d) * tau.powf(t) * (-damping).exp();
        value += term;
        derivative += term * (f64::from(d) - f64::from(c) * damping) / delta;
    }
    // Gaussian terms, rows 52–54. d=3, alpha=20, epsilon=1 for all.
    for (n, t, beta, gamma) in [
        (-0.31306260323435e2, 0, 150.0, 1.21),
        (0.31546140237781e2, 1, 150.0, 1.21),
        (-0.25213154341695e4, 4, 250.0, 1.25),
    ] {
        let term = n
            * delta.powi(3)
            * tau.powi(t)
            * (-20.0 * (delta - 1.0).powi(2) - beta * (tau - gamma).powi(2)).exp();
        value += term;
        derivative += term * (3.0 / delta - 40.0 * (delta - 1.0));
    }
    // Nonanalytic terms, rows 55–56. Retained even though our public dense
    // branch is far from the critical point. No critical-point API is enabled.
    let x = delta - 1.0;
    let q = x * x;
    let theta = (1.0 - tau) + 0.32 * q.powf(1.0 / 0.6);
    let theta_d = (0.32 / 0.3) * x * q.powf(1.0 / 0.6 - 1.0);
    let capital_delta = theta * theta + 0.2 * q.powf(3.5);
    let capital_delta_d = 2.0 * theta * theta_d + 1.4 * x * q.powf(2.5);
    for (n, b, c, d) in [
        (-0.14874640856724, 0.85, 28.0, 700.0),
        (0.31806110878444, 0.95, 32.0, 800.0),
    ] {
        let psi = (-c * q - d * (tau - 1.0).powi(2)).exp();
        let power = capital_delta.powf(b);
        value += n * power * delta * psi;
        derivative += n
            * psi
            * (power * (1.0 - 2.0 * c * x * delta)
                + delta * b * capital_delta.powf(b - 1.0) * capital_delta_d);
    }
    (value, derivative)
}
