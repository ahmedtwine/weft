use std::net::{Ipv4Addr, Ipv6Addr};
use std::ptr;

use hegel_c::{HEGEL_MAX_SIZE_DEFAULT, HegelStringGenerator, hegel_string_t};
use serde_json::{Map, Value as Json};

use crate::engine::{Case, Halt, bytes, checked, text};
use crate::schema::{Bound, Choice, Format};

const DEPTH: usize = 3;
const DOMAIN: u64 = 255;
const NONE: hegel_string_t = hegel_string_t {
    data: ptr::null(),
    len: 0,
};

macro_rules! hegel {
    ($case:expr, $name:ident($($arg:expr),*)) => {
        checked($case.ctx, unsafe { hegel_c::$name($case.ctx, $($arg),*) })
    };
}

struct Drawer<'c> {
    case: &'c Case,
    open: Vec<(&'c str, &'c Choice)>,
    depth: usize,
}

fn python(pattern: &str) -> String {
    let mut out = String::new();
    let mut class = false;
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        let set = match (c, class) {
            ('\\', _) => match chars.next() {
                Some('d') => Some("0-9"),
                Some('w') => Some("A-Za-z0-9_"),
                Some('s') => Some(" \\t\\n\\r\\f\\v"),
                Some(other) => {
                    out.extend(['\\', other]);
                    None
                }
                None => None,
            },
            ('$', false) => {
                out.push_str("\\Z");
                None
            }
            (c, _) => {
                class = match c {
                    '[' => true,
                    ']' => false,
                    _ => class,
                };
                out.push(c);
                None
            }
        };
        match (set, class) {
            (Some(set), true) => out.push_str(set),
            (Some(set), false) => out.extend(["[", set, "]"]),
            (None, _) => {}
        }
    }
    out
}

fn kind(choice: &Choice) -> &'static str {
    match choice {
        Choice::Bool => "weft.bool",
        Choice::Int(..) => "weft.int",
        Choice::Text(..) => "weft.text",
        Choice::Pattern(_) => "weft.pattern",
        Choice::Format(_) => "weft.format",
        Choice::Number(..) => "weft.number",
        Choice::Enum(_) => "weft.enum",
        Choice::AnyOf(_) => "weft.any_of",
        Choice::Array(..) => "weft.array",
        Choice::Tuple(_) => "weft.tuple",
        Choice::Record(..) => "weft.record",
        Choice::Object(_) => "weft.object",
        Choice::Rec(..) => "weft.rec",
        Choice::Var(_) => "weft.var",
    }
}

impl<'c> Drawer<'c> {
    fn last(&self) -> bool {
        self.depth >= DEPTH
    }

    fn most(&self, item: &Choice, min: u64, max: Option<u64>) -> u64 {
        match (self.last() && !item.ends(), item.loops()) {
            (true, _) => min,
            (false, true) => {
                let wide = min + (DEPTH - self.depth) as u64;
                max.map_or(wide, |max| max.min(wide))
            }
            (false, false) => max.unwrap_or(HEGEL_MAX_SIZE_DEFAULT),
        }
    }

    fn index(&self, count: usize) -> Result<usize, Halt> {
        let mut at = 0;
        hegel!(
            self.case,
            hegel_generate_integer(self.case.tc, 0, count as i64 - 1, &mut at)
        )?;
        Ok(at as usize)
    }

    fn coin(&self) -> Result<bool, Halt> {
        let mut heads = false;
        hegel!(
            self.case,
            hegel_generate_boolean(self.case.tc, 0.5, false, false, &mut heads)
        )?;
        Ok(heads)
    }

    fn string(&self, generator: *mut HegelStringGenerator) -> Result<Json, Halt> {
        let mut out = NONE;
        let drawn = hegel!(
            self.case,
            hegel_generate_string(self.case.tc, generator, &mut out)
        );
        hegel!(self.case, hegel_string_generator_free(generator))?;
        drawn?;
        let value = String::from_utf8_lossy(unsafe { bytes(out) }).into_owned();
        hegel!(self.case, hegel_owned_string_free(&mut out))?;
        Ok(Json::from(value))
    }

    fn text(&self, min: u64, max: Option<u64>) -> Result<Json, Halt> {
        let mut generator = ptr::null_mut();
        hegel!(
            self.case,
            hegel_string_generator_text(
                min,
                max.unwrap_or(HEGEL_MAX_SIZE_DEFAULT),
                ptr::null(),
                0,
                u32::MAX,
                ptr::null(),
                0,
                ptr::null(),
                0,
                NONE,
                NONE,
                &mut generator
            )
        )?;
        self.string(generator)
    }

    fn format(&self, format: Format) -> Result<Json, Halt> {
        let mut generator = ptr::null_mut();
        let tc = self.case.tc;
        match format {
            Format::Email => hegel!(self.case, hegel_string_generator_email(&mut generator))?,
            Format::Uri => hegel!(self.case, hegel_string_generator_url(&mut generator))?,
            Format::Hostname => hegel!(
                self.case,
                hegel_string_generator_domain(DOMAIN, &mut generator)
            )?,
            Format::Uuid => {
                let mut out = [0u8; 16];
                hegel!(
                    self.case,
                    hegel_generate_uuid(tc, 0, false, out.as_mut_ptr())
                )?;
                let hex: String = out.iter().map(|b| format!("{b:02x}")).collect();
                return Ok(Json::from(format!(
                    "{}-{}-{}-{}-{}",
                    &hex[..8],
                    &hex[8..12],
                    &hex[12..16],
                    &hex[16..20],
                    &hex[20..]
                )));
            }
            Format::Ipv4 => {
                let mut out = [0u8; 4];
                hegel!(self.case, hegel_generate_ipv4(tc, out.as_mut_ptr()))?;
                return Ok(Json::from(Ipv4Addr::from(out).to_string()));
            }
            Format::Ipv6 => {
                let mut out = [0u8; 16];
                hegel!(self.case, hegel_generate_ipv6(tc, out.as_mut_ptr()))?;
                return Ok(Json::from(Ipv6Addr::from(out).to_string()));
            }
        }
        self.string(generator)
    }

    fn number(&self, min: Bound, max: Bound) -> Result<Json, Halt> {
        let end = |bound: Bound, open: f64| match bound {
            Bound::Open => (open, false),
            Bound::At(n) => (n, false),
            Bound::Past(n) => (n, true),
        };
        let ((low, above), (high, below)) = (end(min, f64::NEG_INFINITY), end(max, f64::INFINITY));
        let mut out = 0.0;
        hegel!(
            self.case,
            hegel_generate_float(
                self.case.tc,
                64,
                low,
                high,
                false,
                false,
                above,
                below,
                f64::from_bits(1),
                &mut out
            )
        )?;
        Ok(Json::from(out))
    }

    fn many(
        &mut self,
        item: &Choice,
        min: u64,
        max: Option<u64>,
        mut one: impl FnMut(&mut Self) -> Result<bool, Halt>,
    ) -> Result<(), Halt> {
        let mut all = ptr::null_mut();
        let tc = self.case.tc;
        hegel!(
            self.case,
            hegel_new_collection(tc, min, self.most(item, min, max), &mut all)
        )?;
        let drawn = (|| {
            loop {
                let mut more = false;
                hegel!(self.case, hegel_collection_more(tc, all, &mut more))?;
                if !more {
                    return Ok(());
                }
                if !one(self)? {
                    hegel!(self.case, hegel_collection_reject(tc, all, ptr::null()))?;
                }
            }
        })();
        hegel!(self.case, hegel_collection_free(all))?;
        drawn
    }

    fn draw(&mut self, choice: &'c Choice) -> Result<Json, Halt> {
        let mut label = 0;
        hegel!(
            self.case,
            hegel_label_from_name(text(kind(choice).as_bytes()), &mut label)
        )?;
        hegel!(self.case, hegel_start_span(self.case.tc, label))?;
        let value = self.value(choice)?;
        hegel!(self.case, hegel_stop_span(self.case.tc, false))?;
        Ok(value)
    }

    fn value(&mut self, choice: &'c Choice) -> Result<Json, Halt> {
        match choice {
            Choice::Bool => self.coin().map(Json::from),
            Choice::Int(min, max) => {
                let mut out = 0;
                hegel!(
                    self.case,
                    hegel_generate_integer(self.case.tc, *min, *max, &mut out)
                )?;
                Ok(Json::from(out))
            }
            Choice::Text(min, max) => self.text(*min, *max),
            Choice::Pattern(pattern) => {
                let mut generator = ptr::null_mut();
                hegel!(
                    self.case,
                    hegel_string_generator_regex(
                        text(python(pattern).as_bytes()),
                        false,
                        ptr::null(),
                        &mut generator
                    )
                )?;
                self.string(generator)
            }
            Choice::Format(format) => self.format(*format),
            Choice::Number(min, max) => self.number(*min, *max),
            Choice::Enum(values) => Ok(values[self.index(values.len())?].clone()),
            Choice::AnyOf(all) => {
                let last = self.last();
                let open: Vec<&Choice> = all.iter().filter(|c| !last || c.ends()).collect();
                let at = self.index(open.len())?;
                self.draw(open[at])
            }
            Choice::Array(item, min, max, unique) => {
                let mut values: Vec<Json> = Vec::new();
                self.many(item, *min, *max, |d| {
                    let value = d.draw(item)?;
                    let fresh = !*unique || !values.contains(&value);
                    if fresh {
                        values.push(value);
                    }
                    Ok(fresh)
                })?;
                Ok(Json::Array(values))
            }
            Choice::Tuple(all) => all.iter().map(|c| self.draw(c)).collect(),
            Choice::Record(key, value) => {
                let mut fields = Map::new();
                self.many(value, 0, None, |d| {
                    let name = match d.draw(key)? {
                        Json::String(name) => name,
                        other => other.to_string(),
                    };
                    let fresh = !fields.contains_key(&name);
                    if fresh {
                        fields.insert(name, d.draw(value)?);
                    }
                    Ok(fresh)
                })?;
                Ok(Json::Object(fields))
            }
            Choice::Object(fields) => {
                let mut drawn = Map::new();
                for (name, field, required) in fields {
                    let held = match (*required, self.last() && !field.ends()) {
                        (true, _) => true,
                        (false, true) => false,
                        (false, false) => self.coin()?,
                    };
                    if held {
                        drawn.insert(name.clone(), self.draw(field)?);
                    }
                }
                Ok(Json::Object(drawn))
            }
            Choice::Rec(key, body, shown) => {
                self.open.push((key.as_str(), &**body));
                let value = self.draw(shown);
                self.open.pop();
                value
            }
            Choice::Var(key) => {
                let body = self
                    .open
                    .iter()
                    .rev()
                    .find(|(k, _)| k == key)
                    .map(|(_, body)| *body)
                    .ok_or_else(|| Halt::Fatal(format!("the schema `{key}` is not open")))?;
                self.depth += 1;
                let value = self.draw(body);
                self.depth -= 1;
                value
            }
        }
    }
}

impl Case {
    pub fn draw(&self, choice: &Choice) -> Result<Json, Halt> {
        Drawer {
            case: self,
            open: Vec::new(),
            depth: 0,
        }
        .draw(choice)
    }
}
