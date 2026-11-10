use std::env;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value as Json;

const ENGINE: &str = "hegeltest-c";
const CONTEXT: &str = "hegel_context_t";
const TEXT: &str = "hegel_string_t";
const RESULT: &str = "hegel_result_t";
const STEERS: [&str; 2] = ["HEGEL_E_RETRY", "HEGEL_E_ASSUME"];
const LOCAL: [&str; 3] = [
    "hegel_context_new",
    "hegel_context_free",
    "hegel_context_last_error",
];
const FIXED: [(&str, usize); 3] = [
    ("hegel_generate_uuid", 16),
    ("hegel_generate_ipv4", 4),
    ("hegel_generate_ipv6", 16),
];
const SCALARS: [(&str, &str); 8] = [
    ("uint8_t", "u8"),
    ("uint32_t", "u32"),
    ("int32_t", "i32"),
    ("uint64_t", "u64"),
    ("int64_t", "i64"),
    ("size_t", "usize"),
    ("double", "f64"),
    ("bool", "bool"),
];

pub struct Param {
    pub ty: String,
    pub name: String,
}

pub struct Call {
    pub name: String,
    pub returns: String,
    pub params: Vec<Param>,
    pub steers: bool,
}

pub struct Plain {
    pub name: String,
    pub fields: Vec<Param>,
}

pub struct Header {
    pub handles: Vec<String>,
    pub enums: Vec<String>,
    pub plain: Vec<Plain>,
    pub calls: Vec<Call>,
    pub print: u32,
}

enum Kind {
    Ctx,
    Handle(usize),
    HandleOut(usize),
    Scalar(&'static str),
    ScalarOut(&'static str),
    EnumOut,
    Text,
    TextOpt,
    Texts,
    List(&'static str),
    TextOut,
    TextOptOut,
    Owned(String),
    Buffer,
    Fixed(usize),
    Plain(String),
    PlainOut(String),
    Callback,
}

type Passed<'h> = (usize, &'h Call, Vec<Arg>);

struct Arg {
    kind: Kind,
    name: String,
    more: Vec<String>,
}

pub fn header() -> Result<PathBuf, String> {
    let cargo = env::var("CARGO").map_err(|e| format!("CARGO: {e}"))?;
    let told = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(
            env::var("CARGO_MANIFEST_DIR").map_err(|e| format!("CARGO_MANIFEST_DIR: {e}"))?,
        )
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    let all: Json = serde_json::from_slice(&told.stdout).map_err(|e| {
        format!(
            "cargo metadata: {e}: {}",
            String::from_utf8_lossy(&told.stderr)
        )
    })?;
    let found: Vec<&str> = all["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["name"] == ENGINE)
        .filter_map(|p| p["manifest_path"].as_str())
        .collect();
    match found.as_slice() {
        [manifest] => Ok(PathBuf::from(manifest)
            .with_file_name("include")
            .join("hegel.h")),
        _ => Err(format!(
            "the workspace has {} versions of `{ENGINE}`, and weft links one",
            found.len()
        )),
    }
}

fn bare(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("/*") {
        out.push_str(&rest[..at]);
        rest = rest[at..].split_once("*/").map_or("", |(_, after)| after);
    }
    out.push_str(rest);
    out.lines()
        .map(|line| line.split("//").next().unwrap_or_default())
        .filter(|line| {
            let line = line.trim_start();
            !line.starts_with('#') && !line.starts_with("extern")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn statements(text: &str) -> Vec<String> {
    let mut all = Vec::new();
    let mut now = String::new();
    let mut depth = 0;
    for c in text.chars() {
        depth += i32::from(c == '{') - i32::from(c == '}');
        if c == ';' && depth == 0 {
            all.push(now.split_whitespace().collect::<Vec<_>>().join(" "));
            now.clear();
        } else {
            now.push(c);
        }
    }
    all
}

fn param(text: &str) -> Param {
    let text = text.trim();
    let at = text
        .rfind(|c: char| !c.is_alphanumeric() && c != '_')
        .map_or(0, |i| i + 1);
    Param {
        ty: text[..at].replace(" *", "*").trim().to_string(),
        name: text[at..].to_string(),
    }
}

fn hash(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

pub fn parse(text: &str) -> Result<Header, String> {
    let mut h = Header {
        handles: Vec::new(),
        enums: Vec::new(),
        plain: Vec::new(),
        calls: Vec::new(),
        print: 0,
    };
    let mut said = String::new();
    for s in statements(&bare(text)) {
        let named = || s.rsplit(['}', ' ']).next().unwrap_or_default().to_string();
        match (
            s.strip_prefix("typedef "),
            s.split_once('{'),
            s.split_once('('),
        ) {
            (Some(body), Some((_, inner)), _) if body.starts_with("struct") => {
                let fields: Vec<Param> = inner
                    .split('}')
                    .next()
                    .unwrap_or_default()
                    .split(';')
                    .filter(|f| !f.trim().is_empty())
                    .map(param)
                    .collect();
                if fields.iter().all(|f| !f.ty.contains('*')) {
                    h.plain.push(Plain {
                        name: named(),
                        fields,
                    });
                }
            }
            (Some(body), Some(_), _) if body.starts_with("enum") => h.enums.push(named()),
            (Some(body), None, None) if body.starts_with("struct") => h.handles.push(named()),
            (Some(_), _, _) => {}
            (None, _, Some((left, inside))) => {
                let Param { ty, name } = param(left);
                let inside = inside.rsplit_once(')').map_or(inside, |(i, _)| i);
                said.push_str(&s);
                let doc = text
                    .find(&format!(" {name}("))
                    .and_then(|at| text[..at].rfind("/*").map(|from| &text[from..at]))
                    .unwrap_or_default();
                h.calls.push(Call {
                    steers: STEERS.iter().any(|code| doc.contains(code)),
                    name,
                    returns: ty,
                    params: match inside.trim() {
                        "void" => Vec::new(),
                        inside => inside.split(',').map(param).collect(),
                    },
                });
            }
            (None, _, None) => {}
        }
    }
    h.print = hash(&said);
    match h.calls.is_empty() {
        true => Err("the header of Hegel has no call".into()),
        false => Ok(h),
    }
}

impl Header {
    fn owned(&self) -> Vec<(&str, &str)> {
        self.calls
            .iter()
            .filter(|c| c.name.ends_with("_free"))
            .filter_map(|c| match c.params.as_slice() {
                [_, p] => {
                    let ty = p.ty.strip_suffix('*')?;
                    (!self.handles.iter().any(|h| h == ty)).then_some((ty, c.name.as_str()))
                }
                _ => None,
            })
            .collect()
    }

    fn local(&self, call: &Call) -> bool {
        LOCAL.contains(&call.name.as_str())
            || self.owned().iter().any(|(_, free)| *free == call.name)
    }

    fn args(&self, call: &Call) -> Result<Vec<Arg>, String> {
        let handle = |ty: &str| self.handles.iter().position(|h| h == ty);
        let scalar = |ty: &str| {
            SCALARS
                .iter()
                .find(|(c, _)| *c == ty)
                .map(|(_, rust)| *rust)
        };
        let plain = |ty: &str| self.plain.iter().any(|p| p.name == ty);
        let counted = |next: Option<&Param>| {
            next.is_some_and(|n| {
                n.ty == "size_t"
                    && (n.name == "len" || n.name.ends_with("_len") || n.name.starts_with("num_"))
            })
        };
        let mut all = Vec::new();
        let mut rest = call.params.as_slice();
        while let [p, after @ ..] = rest {
            let constant = p.ty.starts_with("const ");
            let base = p.ty.trim_start_matches("const ").trim_end_matches('*');
            let stars = p.ty.matches('*').count();
            let sized = matches!(after, [cap, len, ..]
                if cap.ty == "size_t" && cap.name == format!("{}_cap", p.name)
                    && len.ty == "size_t*" && len.name == format!("{}_len", p.name));
            let fixed = FIXED.iter().find(|(name, _)| *name == call.name);
            let (kind, used) = match (base, stars, constant) {
                (CONTEXT, 1, _) => (Kind::Ctx, 0),
                (TEXT, 0, _) => (Kind::Text, 0),
                (TEXT, 1, true) if counted(after.first()) => (Kind::Texts, 1),
                (TEXT, 1, true) => (Kind::TextOpt, 0),
                (TEXT, 1, false) => (Kind::TextOut, 0),
                (TEXT, 2, true) => (Kind::TextOptOut, 0),
                ("hegel_output_callback_t", 0, _)
                    if after.first().is_some_and(|n| n.ty == "void*") =>
                {
                    (Kind::Callback, 1)
                }
                ("uint8_t", 1, false) if sized => (Kind::Buffer, 2),
                ("uint8_t", 1, false) => match fixed {
                    Some((_, size)) => (Kind::Fixed(*size), 0),
                    None => (Kind::ScalarOut("u8"), 0),
                },
                (ty, 1, _) if handle(ty).is_some() => {
                    (Kind::Handle(handle(ty).unwrap_or_default()), 0)
                }
                (ty, 2, false) if handle(ty).is_some() => {
                    (Kind::HandleOut(handle(ty).unwrap_or_default()), 0)
                }
                (ty, 1, false) if self.owned().iter().any(|(owned, _)| *owned == ty) => (
                    Kind::Owned(
                        self.owned()
                            .iter()
                            .find(|(owned, _)| *owned == ty)
                            .map_or(String::new(), |(_, free)| free.to_string()),
                    ),
                    0,
                ),
                (ty, 1, true) if scalar(ty).is_some() && counted(after.first()) => {
                    (Kind::List(scalar(ty).unwrap_or_default()), 1)
                }
                (ty, 0, _) if scalar(ty).is_some() => {
                    (Kind::Scalar(scalar(ty).unwrap_or_default()), 0)
                }
                (ty, 1, false) if scalar(ty).is_some() => {
                    (Kind::ScalarOut(scalar(ty).unwrap_or_default()), 0)
                }
                (ty, 1, false) if self.enums.iter().any(|e| e == ty) => (Kind::EnumOut, 0),
                (ty, 0, _) if plain(ty) => (Kind::Plain(ty.to_string()), 0),
                (ty, 1, false) if plain(ty) => (Kind::PlainOut(ty.to_string()), 0),
                _ => {
                    return Err(format!(
                        "weft passes no parameter `{} {}` of `{}`",
                        p.ty, p.name, call.name
                    ));
                }
            };
            all.push(Arg {
                kind,
                name: format!("p_{}", p.name),
                more: after[..used]
                    .iter()
                    .map(|n| format!("p_{}", n.name))
                    .collect(),
            });
            rest = &after[used..];
        }
        Ok(all)
    }

    fn passed(&self, skip: &[&str]) -> Result<Vec<Passed<'_>>, String> {
        self.calls
            .iter()
            .enumerate()
            .filter(|(_, c)| !self.local(c) && !skip.contains(&c.name.as_str()))
            .map(|(index, call)| match call.returns == RESULT {
                true => Ok((index, call, self.args(call)?)),
                false => Err(format!(
                    "`{}` gives `{}`, and weft passes a call that gives `{RESULT}`",
                    call.name, call.returns
                )),
            })
            .collect()
    }

    fn shared(&self, of: &str, define: bool) -> String {
        let mut out = format!("pub const PRINT: u32 = {};\n", self.print);
        let names = |all: Vec<&String>| {
            all.iter()
                .map(|name| format!("\"{name}\""))
                .collect::<Vec<_>>()
                .join(", ")
        };
        if !define {
            let _ = writeln!(
                out,
                "pub const CALLS: [&str; {}] = [{}];\npub const HANDLES: [&str; {}] = [{}];",
                self.calls.len(),
                names(self.calls.iter().map(|c| &c.name).collect()),
                self.handles.len(),
                names(self.handles.iter().collect())
            );
        }
        for p in &self.plain {
            let rust = |f: &Param| {
                SCALARS
                    .iter()
                    .find(|(c, _)| *c == f.ty)
                    .map_or(f.ty.clone(), |(_, rust)| rust.to_string())
            };
            let nested = |f: &Param| self.plain.iter().any(|p| p.name == f.ty);
            if define {
                let fields: Vec<String> = p
                    .fields
                    .iter()
                    .map(|f| format!("pub {}: {}", f.name, rust(f)))
                    .collect();
                let _ = writeln!(
                    out,
                    "#[repr(C)]\n#[derive(Clone, Copy)]\npub struct {} {{ {} }}",
                    p.name,
                    fields.join(", ")
                );
            }
            let puts: Vec<String> = p
                .fields
                .iter()
                .map(|f| match nested(f) {
                    true => format!("put_{}(w, v.{});", f.ty, f.name),
                    false => format!("w.put(v.{});", f.name),
                })
                .collect();
            let gets: Vec<String> = p
                .fields
                .iter()
                .map(|f| match nested(f) {
                    true => format!("{}: get_{}(r)?", f.name, f.ty),
                    false => format!("{}: r.get()?", f.name),
                })
                .collect();
            let _ = writeln!(
                out,
                "fn put_{0}(w: &mut Writer, v: {of}{0}) {{ {1} }}\nfn get_{0}(r: &mut Reader) -> Result<{of}{0}, Short> {{ Ok({of}{0} {{ {2} }}) }}",
                p.name,
                puts.join(" "),
                gets.join(", ")
            );
        }
        out
    }
}

pub fn client(h: &Header) -> Result<String, String> {
    let mut out = h.shared("", true);
    if let Some(name) = LOCAL
        .iter()
        .find(|name| !h.calls.iter().any(|c| c.name == **name))
    {
        return Err(format!("the header of Hegel has no `{name}`"));
    }
    out.push_str(
        "#[unsafe(no_mangle)]\npub extern \"C\" fn hegel_context_new() -> *mut Context { Box::into_raw(Box::default()) }\n#[unsafe(no_mangle)]\npub unsafe extern \"C\" fn hegel_context_free(ctx: *mut Context) -> i32 { unsafe { freed(ctx) } }\n#[unsafe(no_mangle)]\npub unsafe extern \"C\" fn hegel_context_last_error(ctx: *const Context) -> Text { unsafe { last(ctx) } }\n",
    );
    for (_, free) in h.owned() {
        let _ = writeln!(
            out,
            "#[unsafe(no_mangle)]\npub unsafe extern \"C\" fn {free}(_ctx: *mut Context, result: *mut Text) -> i32 {{ unsafe {{ dropped(result) }} }}"
        );
    }
    for (index, call, args) in h.passed(&[])? {
        let owner = args
            .iter()
            .find(|a| matches!(a.kind, Kind::Handle(_)))
            .map_or("0".to_string(), |a| format!("{} as u64", a.name));
        let freed = call.name.ends_with("_free");
        let mut sig = Vec::new();
        let mut send = String::new();
        let mut read = String::new();
        for Arg {
            kind,
            name: n,
            more,
        } in &args
        {
            let m = |i: usize| more.get(i).cloned().unwrap_or_default();
            match kind {
                Kind::Ctx => sig.push("ctx: *mut Context".to_string()),
                Kind::Handle(_) => {
                    sig.push(format!("{n}: *mut c_void"));
                    let _ = write!(send, "w.put({n} as u64); ");
                }
                Kind::HandleOut(_) => {
                    sig.push(format!("{n}: *mut *mut c_void"));
                    let _ = write!(read, "out({n}, r.get::<u64>()? as *mut c_void); ");
                }
                Kind::Scalar(t) => {
                    sig.push(format!("{n}: {t}"));
                    let _ = write!(send, "w.put({n}); ");
                }
                Kind::ScalarOut(t) => {
                    sig.push(format!("{n}: *mut {t}"));
                    let _ = write!(read, "out({n}, r.get::<{t}>()?); ");
                }
                Kind::EnumOut => {
                    sig.push(format!("{n}: *mut u32"));
                    let _ = write!(read, "out({n}, r.get::<u32>()?); ");
                }
                Kind::Text => {
                    sig.push(format!("{n}: Text"));
                    let _ = write!(send, "w.bytes({n}.bytes()); ");
                }
                Kind::TextOpt => {
                    sig.push(format!("{n}: *const Text"));
                    let _ = write!(send, "w.opt({n}.as_ref().map(|t| t.bytes())); ");
                }
                Kind::Texts => {
                    sig.push(format!("{n}: *const Text, {}: usize", m(0)));
                    let _ = write!(
                        send,
                        "match many({n}, {}) {{ Some(all) => {{ w.put(true); w.put(all.len()); for t in all {{ w.bytes(t.bytes()); }} }} None => w.put(false) }} ",
                        m(0)
                    );
                }
                Kind::List(t) => {
                    sig.push(format!("{n}: *const {t}, {}: usize", m(0)));
                    let _ = write!(
                        send,
                        "match many({n}, {}) {{ Some(all) => {{ w.put(true); w.put(all.len()); for v in all {{ w.put(*v); }} }} None => w.put(false) }} ",
                        m(0)
                    );
                }
                Kind::TextOut => {
                    sig.push(format!("{n}: *mut Text"));
                    let _ = write!(read, "out({n}, kept(owner, r.bytes()?)); ");
                }
                Kind::TextOptOut => {
                    sig.push(format!("{n}: *mut *const Text"));
                    let _ = write!(
                        read,
                        "out({n}, r.opt()?.map_or(ptr::null(), |b| lent(owner, b))); "
                    );
                }
                Kind::Owned(_) => {
                    sig.push(format!("{n}: *mut Text"));
                    let _ = write!(read, "out({n}, given(r.bytes()?)); ");
                }
                Kind::Buffer => {
                    sig.push(format!(
                        "{n}: *mut u8, {}: usize, {}: *mut usize",
                        m(0),
                        m(1)
                    ));
                    let _ = write!(send, "w.put({}); ", m(0));
                    let _ = write!(
                        read,
                        "out({}, r.get::<usize>()?); copied({n}, {}, r.bytes()?); ",
                        m(1),
                        m(0)
                    );
                }
                Kind::Fixed(size) => {
                    sig.push(format!("{n}: *mut u8"));
                    let _ = write!(read, "copied({n}, {size}, r.bytes()?); ");
                }
                Kind::Plain(s) => {
                    sig.push(format!("{n}: {s}"));
                    let _ = write!(send, "put_{s}(&mut w, {n}); ");
                }
                Kind::PlainOut(s) => {
                    sig.push(format!("{n}: *mut {s}"));
                    let _ = write!(read, "out({n}, get_{s}(r)?); ");
                }
                Kind::Callback => {
                    sig.push(format!("{n}: Callback, _{}: *mut c_void", m(0)));
                    let _ = write!(send, "w.put({n}.is_some()); ");
                }
            }
        }
        if freed {
            read.push_str("released(owner); ");
        }
        let owned = match read.contains("owner") {
            true => format!("let owner: u64 = {owner}; "),
            false => String::new(),
        };
        let drawn = read.contains("r.") || read.contains("(r)");
        let pure = drawn
            && args.iter().all(|a| {
                matches!(
                    a.kind,
                    Kind::Ctx | Kind::Text | Kind::Scalar(_) | Kind::List(_) | Kind::ScalarOut(_)
                )
            });
        let tail = match (drawn || call.steers, pure) {
            (false, _) => format!("{read}deferred(ctx, w)"),
            (true, true) => format!("pure(ctx, w, |r| {{ {read}Ok(()) }})"),
            (true, false) => format!(
                "asked(ctx, w, |{}r| {{ {read}Ok(()) }})",
                if drawn { "" } else { "_" }
            ),
        };
        let _ = writeln!(
            out,
            "#[unsafe(no_mangle)]\npub unsafe extern \"C\" fn {}({}) -> i32 {{ unsafe {{ let {}w = call({index}); {send}{owned}{tail} }} }}",
            call.name,
            sig.join(", "),
            if send.is_empty() { "" } else { "mut " }
        );
    }
    Ok(out)
}

pub fn server(h: &Header, skip: &[&str]) -> Result<String, String> {
    let mut out = h.shared("hegel_c::", false);
    out.push_str(
        "pub fn dispatch(host: &mut Host, call: u16, r: &mut Reader, w: &mut Writer) -> Result<i32, Short> {\nmatch call {\n",
    );
    for (index, call, args) in h.passed(skip)? {
        let freed = call.name.ends_with("_free");
        let mut prepare = String::new();
        let mut pass = Vec::new();
        let mut after = String::new();
        for Arg {
            kind,
            name: n,
            more,
        } in &args
        {
            let m = |i: usize| more.get(i).cloned().unwrap_or_default();
            match kind {
                Kind::Ctx => pass.push("host.ctx()".to_string()),
                Kind::Handle(k) if freed => {
                    let _ = write!(
                        prepare,
                        "let Some({n}) = host.take({k}, r.get::<u64>()?)? else {{ return Ok(0) }}; "
                    );
                    pass.push(format!("{n} as _"));
                }
                Kind::Handle(k) => {
                    let _ = write!(prepare, "let {n} = host.handle({k}, r.get::<u64>()?)?; ");
                    pass.push(format!("{n} as _"));
                }
                Kind::HandleOut(k) => {
                    let _ = write!(prepare, "let mut {n} = MaybeUninit::uninit(); ");
                    pass.push(format!("{n}.as_mut_ptr()"));
                    let _ = write!(
                        after,
                        "w.put(host.insert({k}, unsafe {{ {n}.assume_init() }} as *mut c_void)); "
                    );
                }
                Kind::Scalar(t) => {
                    let _ = write!(prepare, "let {n} = r.get::<{t}>()?; ");
                    pass.push(n.clone());
                }
                Kind::ScalarOut(t) => {
                    let _ = write!(prepare, "let mut {n} = MaybeUninit::<{t}>::uninit(); ");
                    pass.push(format!("{n}.as_mut_ptr()"));
                    let _ = write!(after, "w.put(unsafe {{ {n}.assume_init() }}); ");
                }
                Kind::EnumOut => {
                    let _ = write!(prepare, "let mut {n} = MaybeUninit::uninit(); ");
                    pass.push(format!("{n}.as_mut_ptr()"));
                    let _ = write!(after, "w.put(unsafe {{ {n}.assume_init() }} as u32); ");
                }
                Kind::Text => {
                    let _ = write!(prepare, "let {n} = text(r.bytes()?); ");
                    pass.push(n.clone());
                }
                Kind::TextOpt => {
                    let _ = write!(prepare, "let {n} = r.opt()?.map(text); ");
                    pass.push(format!(
                        "{n}.as_ref().map_or(ptr::null(), |t| t as *const _)"
                    ));
                }
                Kind::Texts => {
                    let _ = write!(
                        prepare,
                        "let {n} = match r.get::<bool>()? {{ true => Some((0..r.get::<usize>()?).map(|_| r.bytes().map(text)).collect::<Result<Vec<_>, Short>>()?), false => None }}; "
                    );
                    pass.push(format!(
                        "{n}.as_ref().map_or(ptr::null(), |all| all.as_ptr())"
                    ));
                    pass.push(format!("{n}.as_ref().map_or(0, Vec::len)"));
                }
                Kind::List(t) => {
                    let _ = write!(
                        prepare,
                        "let {n} = match r.get::<bool>()? {{ true => Some((0..r.get::<usize>()?).map(|_| r.get::<{t}>()).collect::<Result<Vec<_>, Short>>()?), false => None }}; "
                    );
                    pass.push(format!(
                        "{n}.as_ref().map_or(ptr::null(), |all| all.as_ptr())"
                    ));
                    pass.push(format!("{n}.as_ref().map_or(0, Vec::len)"));
                }
                Kind::TextOut => {
                    let _ = write!(
                        prepare,
                        "let mut {n} = MaybeUninit::<hegel_c::hegel_string_t>::uninit(); "
                    );
                    pass.push(format!("{n}.as_mut_ptr()"));
                    let _ = write!(after, "w.bytes(unsafe {{ bytes({n}.assume_init()) }}); ");
                }
                Kind::TextOptOut => {
                    let _ = write!(
                        prepare,
                        "let mut {n} = MaybeUninit::<*const hegel_c::hegel_string_t>::uninit(); "
                    );
                    pass.push(format!("{n}.as_mut_ptr()"));
                    let _ = write!(
                        after,
                        "w.opt(unsafe {{ {n}.assume_init().as_ref() }}.map(|t| unsafe {{ bytes(*t) }})); "
                    );
                }
                Kind::Owned(free) => {
                    let _ = write!(prepare, "let mut {n} = MaybeUninit::zeroed(); ");
                    pass.push(format!("{n}.as_mut_ptr()"));
                    let _ = write!(
                        after,
                        "let mut held = unsafe {{ {n}.assume_init() }}; w.bytes(unsafe {{ raw(held.data.cast(), held.len) }}); let _ = unsafe {{ hegel_c::{free}(host.ctx(), &mut held) }}; "
                    );
                }
                Kind::Buffer => {
                    let _ = write!(
                        prepare,
                        "let {0} = r.get::<usize>()?.min(BUFFER); let mut {n} = vec![0u8; {0}]; let mut {1} = 0usize; ",
                        m(0),
                        m(1)
                    );
                    pass.push(format!("{n}.as_mut_ptr()"));
                    pass.push(m(0));
                    pass.push(format!("&mut {}", m(1)));
                    let _ = write!(
                        after,
                        "w.put({1}); w.bytes(&{n}[..{1}.min({0})]); ",
                        m(0),
                        m(1)
                    );
                }
                Kind::Fixed(size) => {
                    let _ = write!(prepare, "let mut {n} = [0u8; {size}]; ");
                    pass.push(format!("{n}.as_mut_ptr()"));
                    let _ = write!(after, "w.bytes(&{n}); ");
                }
                Kind::Plain(s) => {
                    let _ = write!(prepare, "let {n} = get_{s}(r)?; ");
                    pass.push(n.clone());
                }
                Kind::PlainOut(s) => {
                    let _ = write!(prepare, "let mut {n} = MaybeUninit::uninit(); ");
                    pass.push(format!("{n}.as_mut_ptr()"));
                    let _ = write!(after, "put_{s}(w, unsafe {{ {n}.assume_init() }}); ");
                }
                Kind::Callback => {
                    return Err(format!(
                        "`{}` takes an output callback, and weft owns each call that takes one",
                        call.name
                    ));
                }
            }
        }
        let _ = writeln!(
            out,
            "{index} => {{ {prepare}let code = unsafe {{ hegel_c::{}({}) }} as i32; {}Ok(code) }}",
            call.name,
            pass.join(", "),
            match after.is_empty() {
                true => String::new(),
                false => format!("if code == 0 {{ {after}}} "),
            }
        );
    }
    out.push_str("_ => Err(Short),\n}\n}\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn real() -> Header {
        let path = header().unwrap_or_else(|e| panic!("{e}"));
        let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
        parse(&text).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn the_client_passes_each_call_of_the_header_of_hegel() {
        let h = real();
        let made = client(&h).unwrap_or_else(|e| panic!("{e}"));
        let missing: Vec<&str> = h
            .calls
            .iter()
            .map(|c| c.name.as_str())
            .filter(|name| !made.contains(&format!("fn {name}(")))
            .collect();
        assert!(missing.is_empty(), "{missing:?}");
    }

    #[test]
    fn the_server_answers_each_call_that_weft_does_not_own() {
        let h = real();
        let owned = [
            "hegel_run_start",
            "hegel_run_start_blob",
            "hegel_test_case_from_blob",
        ];
        let made = server(&h, &owned).unwrap_or_else(|e| panic!("{e}"));
        assert!(made.contains("hegel_c::hegel_generate_integer("));
        assert!(made.contains("hegel_c::hegel_pool_generate("));
        assert!(!made.contains("hegel_c::hegel_run_start("));
    }

    #[test]
    fn a_call_whose_code_steers_the_generator_is_never_batched() {
        let h = real();
        let made = client(&h).unwrap_or_else(|e| panic!("{e}"));
        let line = |name: &str| {
            made.lines()
                .find(|l| l.contains(&format!("fn {name}(")))
                .unwrap_or_default()
                .to_string()
        };
        assert!(line("hegel_recursion_leaf").contains("asked("));
        assert!(line("hegel_stop_span").contains("deferred("));
        assert!(line("hegel_label_from_name").contains("pure("));
    }

    #[test]
    fn a_change_of_the_header_changes_the_print() {
        let text = fs::read_to_string(header().unwrap_or_else(|e| panic!("{e}")))
            .unwrap_or_else(|e| panic!("{e}"));
        let changed = text.replace("uint64_t max_size", "uint64_t most_size");
        let (a, b) = (parse(&text), parse(&changed));
        assert_ne!(a.map(|h| h.print).ok(), b.map(|h| h.print).ok());
    }
}
