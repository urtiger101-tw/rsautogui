/// Easing: t in [0, 1] → [0, 1] (elastic/back/bounce may overshoot mid-range).
pub type Tween = fn(f64) -> f64;

fn clamp_t(t: f64) -> f64 {
    t.clamp(0.0, 1.0)
}

pub fn linear(t: f64) -> f64 {
    clamp_t(t)
}

pub fn ease_in_quad(t: f64) -> f64 {
    let t = clamp_t(t);
    t * t
}

pub fn ease_out_quad(t: f64) -> f64 {
    let t = clamp_t(t);
    t * (2.0 - t)
}

pub fn ease_in_out_quad(t: f64) -> f64 {
    let t = clamp_t(t);
    if t < 0.5 {
        2.0 * t * t
    } else {
        -1.0 + (4.0 - 2.0 * t) * t
    }
}

pub fn ease_in_cubic(t: f64) -> f64 {
    let t = clamp_t(t);
    t * t * t
}

pub fn ease_out_cubic(t: f64) -> f64 {
    let t = clamp_t(t);
    let u = t - 1.0;
    u * u * u + 1.0
}

pub fn ease_in_out_cubic(t: f64) -> f64 {
    let t = clamp_t(t);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        let u = 2.0 * t - 2.0;
        0.5 * u * u * u + 1.0
    }
}

pub fn ease_in_quart(t: f64) -> f64 {
    let t = clamp_t(t);
    t.powi(4)
}

pub fn ease_out_quart(t: f64) -> f64 {
    let t = clamp_t(t);
    1.0 - (t - 1.0).powi(4)
}

pub fn ease_in_out_quart(t: f64) -> f64 {
    let t = clamp_t(t);
    if t < 0.5 {
        8.0 * t.powi(4)
    } else {
        1.0 - (-2.0 * t + 2.0).powi(4) / 2.0
    }
}

pub fn ease_in_quint(t: f64) -> f64 {
    let t = clamp_t(t);
    t.powi(5)
}

pub fn ease_out_quint(t: f64) -> f64 {
    let t = clamp_t(t);
    1.0 + (t - 1.0).powi(5)
}

pub fn ease_in_out_quint(t: f64) -> f64 {
    let t = clamp_t(t);
    if t < 0.5 {
        16.0 * t.powi(5)
    } else {
        1.0 + (-2.0 * t + 2.0).powi(5) / 2.0
    }
}

pub fn ease_in_sine(t: f64) -> f64 {
    let t = clamp_t(t);
    1.0 - (t * std::f64::consts::FRAC_PI_2).cos()
}

pub fn ease_out_sine(t: f64) -> f64 {
    let t = clamp_t(t);
    (t * std::f64::consts::FRAC_PI_2).sin()
}

pub fn ease_in_out_sine(t: f64) -> f64 {
    let t = clamp_t(t);
    -(std::f64::consts::PI * t).cos() / 2.0 + 0.5
}

pub fn ease_in_expo(t: f64) -> f64 {
    let t = clamp_t(t);
    if t == 0.0 {
        0.0
    } else {
        (2.0_f64).powf(10.0 * t - 10.0)
    }
}

pub fn ease_out_expo(t: f64) -> f64 {
    let t = clamp_t(t);
    if t == 1.0 {
        1.0
    } else {
        1.0 - (2.0_f64).powf(-10.0 * t)
    }
}

pub fn ease_in_out_expo(t: f64) -> f64 {
    let t = clamp_t(t);
    if t == 0.0 {
        0.0
    } else if t == 1.0 {
        1.0
    } else if t < 0.5 {
        (2.0_f64).powf(20.0 * t - 10.0) / 2.0
    } else {
        (2.0 - (2.0_f64).powf(-20.0 * t + 10.0)) / 2.0
    }
}

pub fn ease_in_circ(t: f64) -> f64 {
    let t = clamp_t(t);
    1.0 - (1.0 - t * t).sqrt()
}

pub fn ease_out_circ(t: f64) -> f64 {
    let t = clamp_t(t);
    (1.0 - (t - 1.0).powi(2)).sqrt()
}

pub fn ease_in_out_circ(t: f64) -> f64 {
    let t = clamp_t(t);
    if t < 0.5 {
        (1.0 - (1.0 - (2.0 * t).powi(2)).sqrt()) / 2.0
    } else {
        ((1.0 - (-2.0 * t + 2.0).powi(2)).sqrt() + 1.0) / 2.0
    }
}

pub fn ease_in_elastic(t: f64) -> f64 {
    let t = clamp_t(t);
    if t == 0.0 || t == 1.0 {
        t
    } else {
        let c4 = (2.0 * std::f64::consts::PI) / 3.0;
        -(2.0_f64).powf(10.0 * t - 10.0) * ((t * 10.0 - 10.75) * c4).sin()
    }
}

pub fn ease_out_elastic(t: f64) -> f64 {
    let t = clamp_t(t);
    if t == 0.0 || t == 1.0 {
        t
    } else {
        let c4 = (2.0 * std::f64::consts::PI) / 3.0;
        (2.0_f64).powf(-10.0 * t) * ((t * 10.0 - 0.75) * c4).sin() + 1.0
    }
}

pub fn ease_in_out_elastic(t: f64) -> f64 {
    let t = clamp_t(t);
    if t == 0.0 || t == 1.0 {
        t
    } else {
        let c5 = (2.0 * std::f64::consts::PI) / 4.5;
        if t < 0.5 {
            -((2.0_f64).powf(20.0 * t - 10.0) * ((20.0 * t - 11.125) * c5).sin()) / 2.0
        } else {
            ((2.0_f64).powf(-20.0 * t + 10.0) * ((20.0 * t - 11.125) * c5).sin()) / 2.0 + 1.0
        }
    }
}

pub fn ease_in_back(t: f64) -> f64 {
    let t = clamp_t(t);
    const C1: f64 = 1.70158;
    const C3: f64 = C1 + 1.0;
    C3 * t * t * t - C1 * t * t
}

pub fn ease_out_back(t: f64) -> f64 {
    let t = clamp_t(t);
    const C1: f64 = 1.70158;
    const C3: f64 = C1 + 1.0;
    1.0 + C3 * (t - 1.0).powi(3) + C1 * (t - 1.0).powi(2)
}

pub fn ease_in_out_back(t: f64) -> f64 {
    let t = clamp_t(t);
    const C1: f64 = 1.70158;
    const C2: f64 = C1 * 1.525;
    if t < 0.5 {
        ((2.0 * t).powi(2) * ((C2 + 1.0) * 2.0 * t - C2)) / 2.0
    } else {
        ((2.0 * t - 2.0).powi(2) * ((C2 + 1.0) * (t * 2.0 - 2.0) + C2) + 2.0) / 2.0
    }
}

fn bounce_out(t: f64) -> f64 {
    const N1: f64 = 7.5625;
    const D1: f64 = 2.75;
    if t < 1.0 / D1 {
        N1 * t * t
    } else if t < 2.0 / D1 {
        let t = t - 1.5 / D1;
        N1 * t * t + 0.75
    } else if t < 2.5 / D1 {
        let t = t - 2.25 / D1;
        N1 * t * t + 0.9375
    } else {
        let t = t - 2.625 / D1;
        N1 * t * t + 0.984375
    }
}

pub fn ease_in_bounce(t: f64) -> f64 {
    let t = clamp_t(t);
    if t == 0.0 || t == 1.0 {
        t
    } else {
        1.0 - bounce_out(1.0 - t)
    }
}

pub fn ease_out_bounce(t: f64) -> f64 {
    let t = clamp_t(t);
    if t == 0.0 || t == 1.0 {
        t
    } else {
        bounce_out(t)
    }
}

pub fn ease_in_out_bounce(t: f64) -> f64 {
    let t = clamp_t(t);
    if t == 0.0 || t == 1.0 {
        t
    } else if t < 0.5 {
        (1.0 - bounce_out(1.0 - 2.0 * t)) / 2.0
    } else {
        (1.0 + bounce_out(2.0 * t - 1.0)) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ends(f: Tween) {
        assert!((f(0.0) - 0.0).abs() < 1e-9);
        assert!((f(1.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn endpoints() {
        for f in [
            linear as Tween,
            ease_in_quad,
            ease_out_quad,
            ease_in_out_quad,
            ease_in_cubic,
            ease_out_cubic,
            ease_in_out_cubic,
            ease_in_quart,
            ease_out_quart,
            ease_in_out_quart,
            ease_in_quint,
            ease_out_quint,
            ease_in_out_quint,
            ease_in_sine,
            ease_out_sine,
            ease_in_out_sine,
            ease_in_expo,
            ease_out_expo,
            ease_in_out_expo,
            ease_in_circ,
            ease_out_circ,
            ease_in_out_circ,
            ease_in_elastic,
            ease_out_elastic,
            ease_in_out_elastic,
            ease_in_back,
            ease_out_back,
            ease_in_out_back,
            ease_in_bounce,
            ease_out_bounce,
            ease_in_out_bounce,
        ] {
            ends(f);
        }
    }
}
