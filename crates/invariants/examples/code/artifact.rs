mod domain {
    pub struct Finding {
        pub core: Vec<usize>,
        pub relieves: Vec<String>,
        pub keeps: Vec<String>,
    }
}

mod models {
    pub struct Separation {
        pub lowest_bad: f64,
        pub highest_good: f64,
        pub separated: bool,
    }

    pub fn known(rules: &[String], unit: &str) -> Vec<usize> {
        // keep the index of each rule that the unit contains
        let hits = rules.iter().enumerate().filter(|(_, r)| unit.contains(r.as_str())).map(|(i, _)| i);
        hits.collect()
    }
}

mod check {
    pub fn measure(bad: &[f64], good: &[f64]) -> f64 {
        let lowest = bad.iter().copied().fold(1.0, f64::min);
        let highest = good.iter().copied().fold(0.0, f64::max);
        (lowest + highest) / 2.0
    }
}
