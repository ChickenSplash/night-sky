//! Direct evaluation of IAPWS R7-97(2012), region 1, equation 7 / table 2.
//! Coefficients attributed to IAPWS. No interpolation, inversion or extrapolation.

pub(super) fn density_kg_m3(pressure_pa: f64, temperature_k: f64) -> f64 {
    let pi = pressure_pa / 16.53e6;
    let tau = 1386.0 / temperature_k;
    let gamma_pi = TERMS
        .iter()
        .filter(|(i, _, _)| *i != 0)
        .map(|&(i, j, n)| -n * f64::from(i) * (7.1 - pi).powi(i - 1) * (tau - 1.222).powi(j))
        .sum::<f64>();
    // v = RT/p* times gamma_pi. The IF97-specific gas constant is part of
    // its fitted formulation, not a replacement for the project's SI constants.
    16.53e6 / (461.526 * temperature_k * gamma_pi)
}

/// Region 4 saturation pressure, equation 30 / table 34. Internal callers
/// enforce 273.16 < T <=623.15 K, below the critical point.
pub(super) fn saturation_pressure_pa(temperature_k: f64) -> f64 {
    let theta = temperature_k - 0.23855557567849 / (temperature_k - 650.17534844798);
    let a = theta.powi(2) + 1167.0521452767 * theta - 724213.16703206;
    let b = -17.073846940092 * theta.powi(2) + 12020.824702470 * theta - 3232555.0322333;
    let c = 14.915108613530 * theta.powi(2) - 4823.2657361591 * theta + 405113.40542057;
    1e6 * (2.0 * c / (-b + (b * b - 4.0 * a * c).sqrt())).powi(4)
}

const TERMS: [(i32, i32, f64); 34] = [
    (0, -2, 0.14632971213167),
    (0, -1, -0.84548187169114),
    (0, 0, -0.37563603672040e1),
    (0, 1, 0.33855169168385e1),
    (0, 2, -0.95791963387872),
    (0, 3, 0.15772038513228),
    (0, 4, -0.16616417199501e-1),
    (0, 5, 0.81214629983568e-3),
    (1, -9, 0.28319080123804e-3),
    (1, -7, -0.60706301565874e-3),
    (1, -1, -0.18990068218419e-1),
    (1, 0, -0.32529748770505e-1),
    (1, 1, -0.21841717175414e-1),
    (1, 3, -0.52838357969930e-4),
    (2, -3, -0.47184321073267e-3),
    (2, 0, -0.30001780793026e-3),
    (2, 1, 0.47661393906987e-4),
    (2, 3, -0.44141845330846e-5),
    (2, 17, -0.72694996297594e-15),
    (3, -4, -0.31679644845054e-4),
    (3, 0, -0.28270797985312e-5),
    (3, 6, -0.85205128120103e-9),
    (4, -5, -0.22425281908000e-5),
    (4, -2, -0.65171222895601e-6),
    (4, 10, -0.14341729937924e-12),
    (5, -8, -0.40516996860117e-6),
    (8, -11, -0.12734301741641e-8),
    (8, -6, -0.17424871230634e-9),
    (21, -29, -0.68762131295531e-18),
    (23, -31, 0.14478307828521e-19),
    (29, -38, 0.26335781662795e-22),
    (30, -39, -0.11947622640071e-22),
    (31, -40, 0.18228094581404e-23),
    (32, -41, -0.93537087292458e-25),
];
