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
