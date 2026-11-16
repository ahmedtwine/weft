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

pub const MAX_ITEMS: usize = 50;
pub const MAX_PEOPLE: usize = 50;
pub const ITEM_LIMIT: usize = 50;

pub struct Item {
    pub name: String,
    pub cents: u64,
}

pub struct Manager {
    pub data: Vec<Item>,
}

pub fn subtotal(items: &[Item]) -> u64 {
    // add up the cents of each item
    items.iter().map(|i| i.cents).sum()
}

pub fn get_subtotal(items: &[Item]) -> u64 {
    subtotal(items)
}

pub fn receipt(items: &[Item]) -> String {
    format!("{} items, ", items.len()) + &get_subtotal(items).to_string() + " cents"
}

pub fn fits(items: &[Item]) -> bool {
    let basket = items;
    basket.len() <= ITEM_LIMIT && items.len() <= MAX_ITEMS
}

pub fn process(data: &[Item]) -> Vec<String> {
    data.iter().map(|i| i.name.clone()).collect()
}

pub fn share(cents: u64, people: u64) -> Result<u64, String> {
    if people == 0 || people > MAX_PEOPLE as u64 {
        return Err(format!("a share needs 1 to {MAX_PEOPLE} people"));
    }
    // the shop keeps the remainder, so each share rounds down
    Ok(cents / people)
}
