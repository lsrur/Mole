//! Formateador de llaves del host — spec §7.2 (REC-32..REC-39, TEST-08).
//!
//! Sintaxis: `{}`, `{:.2}`, `{:04X}`, `{{`/`}}` como escape. El string lleva
//! solo presentación; el tipo lo aportan los argumentos ya tipados. Cualquier
//! malformación renderiza un placeholder `{!...}` — nunca panic (SEC-04):
//! esto formatea datos que vienen del wire.

use mole_codec::args::Value;
use mole_codec::types::TypeDef;

/// Resuelve nombres para símbolos internados y descriptores de tipo.
pub trait Resolver {
    fn sym_name(&self, sym_id: u16) -> Option<String>;
    fn type_def(&self, type_id: u16) -> Option<&TypeDef>;
}

/// Resolver vacío: los símbolos se muestran como `<sym#N>`.
pub struct NoResolver;

impl Resolver for NoResolver {
    fn sym_name(&self, _: u16) -> Option<String> {
        None
    }
    fn type_def(&self, _: u16) -> Option<&TypeDef> {
        None
    }
}

impl Resolver for mole_codec::catalog::Catalog {
    fn sym_name(&self, sym_id: u16) -> Option<String> {
        self.sym(sym_id)
            .map(|s| String::from_utf8_lossy(&s.name).into_owned())
    }
    fn type_def(&self, type_id: u16) -> Option<&TypeDef> {
        self.types.get(type_id)
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct Spec {
    plus: bool,
    alt: bool,      // '#': prefijo 0x/0b/0o, o expansión de struct
    zero: bool,     // relleno con ceros
    width: usize,
    precision: Option<usize>,
    ty: Option<char>, // x, X, b, o
}

fn parse_spec(s: &str) -> Option<Spec> {
    let mut spec = Spec::default();
    let mut it = s.chars().peekable();
    if it.peek() == Some(&'+') {
        spec.plus = true;
        it.next();
    }
    if it.peek() == Some(&'#') {
        spec.alt = true;
        it.next();
    }
    if it.peek() == Some(&'0') {
        spec.zero = true;
        it.next();
    }
    while let Some(c) = it.peek() {
        if c.is_ascii_digit() {
            spec.width = spec.width * 10 + (*c as usize - '0' as usize);
            it.next();
        } else {
            break;
        }
    }
    if it.peek() == Some(&'.') {
        it.next();
        let mut p = 0usize;
        let mut any = false;
        while let Some(c) = it.peek() {
            if c.is_ascii_digit() {
                p = p * 10 + (*c as usize - '0' as usize);
                any = true;
                it.next();
            } else {
                break;
            }
        }
        if !any {
            return None;
        }
        spec.precision = Some(p);
    }
    if let Some(c) = it.next() {
        match c {
            'x' | 'X' | 'b' | 'o' => spec.ty = Some(c),
            _ => return None,
        }
    }
    if it.next().is_some() {
        return None;
    }
    Some(spec)
}

/// Relleno a `width`: ceros después del signo, espacios a la izquierda.
fn pad(s: String, spec: &Spec) -> String {
    if s.len() >= spec.width {
        return s;
    }
    let fill = spec.width - s.len();
    if spec.zero {
        let (sign, digits) = match s.strip_prefix('-') {
            Some(d) => ("-", d),
            None => match s.strip_prefix('+') {
                Some(d) => ("+", d),
                None => ("", s.as_str()),
            },
        };
        format!("{sign}{}{digits}", "0".repeat(fill))
    } else {
        format!("{}{s}", " ".repeat(fill))
    }
}

/// (valor con signo, bits crudos, ancho en bits) de un entero.
fn int_parts(v: &Value) -> Option<(i128, u128, u32)> {
    Some(match v {
        Value::U8(x) => (*x as i128, *x as u128, 8),
        Value::I8(x) => (*x as i128, *x as u8 as u128, 8),
        Value::U16(x) | Value::Sym(x) => (*x as i128, *x as u128, 16),
        Value::I16(x) => (*x as i128, *x as u16 as u128, 16),
        Value::U32(x) | Value::Ptr(x) => (*x as i128, *x as u128, 32),
        Value::I32(x) => (*x as i128, *x as u32 as u128, 32),
        Value::U64(x) => (*x as i128, *x as u128, 64),
        Value::I64(x) => (*x as i128, *x as u64 as u128, 64),
        Value::Bool(x) => (*x as i128, *x as u128, 8),
        _ => return None,
    })
}

fn render_int(v: &Value, spec: &Spec) -> Option<String> {
    let (signed, bits, _) = int_parts(v)?;
    let body = match spec.ty {
        None => {
            let mut s = signed.to_string();
            if spec.plus && signed >= 0 {
                s = format!("+{s}");
            }
            s
        }
        // En bases no decimales se formatean los bits crudos (complemento a
        // dos dentro del ancho del tipo), como el formateador de Rust.
        Some('x') => format!("{}{:x}", if spec.alt { "0x" } else { "" }, bits),
        Some('X') => format!("{}{:X}", if spec.alt { "0x" } else { "" }, bits),
        Some('b') => format!("{}{:b}", if spec.alt { "0b" } else { "" }, bits),
        Some('o') => format!("{}{:o}", if spec.alt { "0o" } else { "" }, bits),
        _ => return None,
    };
    Some(pad(body, spec))
}

fn render_float(f: f64, spec: &Spec) -> Option<String> {
    if spec.ty.is_some() {
        return None; // hex de un float no existe: placeholder
    }
    let mut s = match spec.precision {
        Some(p) => format!("{f:.p$}"),
        None => format!("{f}"),
    };
    if spec.plus && !s.starts_with('-') {
        s = format!("+{s}");
    }
    Some(pad(s, spec))
}

fn render_struct(type_id: u16, fields: &[Value], spec: &Spec, res: &dyn Resolver) -> String {
    let (name, field_names) = match res.type_def(type_id) {
        Some(def) => (
            String::from_utf8_lossy(&def.name).into_owned(),
            def.fields
                .iter()
                .map(|f| res.sym_name(f.name_sym))
                .collect::<Vec<_>>(),
        ),
        None => (format!("<type#{type_id}>"), Vec::new()),
    };
    let mut parts = Vec::with_capacity(fields.len());
    for (i, v) in fields.iter().enumerate() {
        let fname = field_names
            .get(i)
            .cloned()
            .flatten()
            .unwrap_or_else(|| format!("f{i}"));
        parts.push(format!("{fname}={}", render_value(v, &Spec::default(), res)));
    }
    // `{:#}` (REC-40) pide renderizado expandido; en texto plano equivale al
    // compacto — la expansión a tabla es de la UI.
    let _ = spec.alt;
    format!("{name}{{{}}}", parts.join(", "))
}

fn render_value(v: &Value, spec: &Spec, res: &dyn Resolver) -> String {
    let rendered = match v {
        Value::F32(f) => render_float(*f as f64, spec),
        Value::F64(f) => render_float(*f, spec),
        Value::Bool(b) => {
            if spec.ty.is_some() {
                render_int(v, spec)
            } else {
                Some(pad(b.to_string(), spec))
            }
        }
        Value::Sym(id) => match spec.ty {
            // un sym con base numérica se formatea como su id
            Some(_) => render_int(v, spec),
            None => Some(pad(
                res.sym_name(*id).unwrap_or_else(|| format!("<sym#{id}>")),
                spec,
            )),
        },
        Value::Str(bytes) => Some(pad(
            String::from_utf8_lossy(bytes).into_owned(),
            spec,
        )),
        Value::Ptr(p) => match spec.ty {
            Some(_) => render_int(v, spec),
            None => Some(pad(format!("0x{p:08x}"), spec)),
        },
        Value::Struct { type_id, fields } => Some(render_struct(*type_id, fields, spec, res)),
        _ => render_int(v, spec),
    };
    rendered.unwrap_or_else(|| "{!spec}".to_string())
}

/// Renderiza un format string de llaves con sus argumentos ya decodificados.
///
/// Degradación sin panic (TEST-08):
/// - agujero sin argumento → `{!argN}`
/// - spec malformada → `{!spec}`
/// - `{` sin cerrar → `{!fmt}` y se corta
///
/// Los argumentos sobrantes se ignoran: la verificación fuerte ocurre antes,
/// en `decode_args` (`arg_count_mismatch`).
pub fn format(fmt: &str, args: &[Value], res: &dyn Resolver) -> String {
    let mut out = String::with_capacity(fmt.len() + args.len() * 8);
    let mut chars = fmt.chars().peekable();
    let mut arg_i = 0usize;
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    out.push('{');
                    continue;
                }
                // juntar hasta '}'
                let mut spec_txt = String::new();
                let mut closed = false;
                for c2 in chars.by_ref() {
                    if c2 == '}' {
                        closed = true;
                        break;
                    }
                    spec_txt.push(c2);
                }
                if !closed {
                    out.push_str("{!fmt}");
                    break;
                }
                let spec = if spec_txt.is_empty() {
                    Some(Spec::default())
                } else if let Some(rest) = spec_txt.strip_prefix(':') {
                    parse_spec(rest)
                } else {
                    None // `{algo}` sin ':' no existe en esta sintaxis
                };
                match (spec, args.get(arg_i)) {
                    (Some(sp), Some(v)) => out.push_str(&render_value(v, &sp, res)),
                    (Some(_), None) => out.push_str(&format!("{{!arg{arg_i}}}")),
                    (None, _) => out.push_str("{!spec}"),
                }
                arg_i += 1;
            }
            '}' => {
                if chars.peek() == Some(&'}') {
                    chars.next();
                }
                out.push('}');
            }
            c => out.push(c),
        }
    }
    out
}
