use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct ParsedArgs {
    values: HashMap<String, Vec<String>>,
    bools: HashMap<String, bool>,
    pub positionals: Vec<String>,
}

impl ParsedArgs {
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.values
            .get(name)
            .and_then(|v| v.last())
            .map(String::as_str)
    }

    #[must_use]
    pub fn get_all(&self, name: &str) -> &[String] {
        self.values.get(name).map_or(&[], Vec::as_slice)
    }

    #[must_use]
    pub fn get_bool(&self, name: &str, default: bool) -> bool {
        self.bools.get(name).copied().unwrap_or(default)
    }

    pub fn parse<'a>(
        args: impl IntoIterator<Item = String>,
        value_flags: &[&'a str],
        bool_flags: &[&'a str],
    ) -> Result<Self, String> {
        let mut out = Self::default();
        let mut iter = args.into_iter().skip(1);
        while let Some(arg) = iter.next() {
            if !arg.starts_with('-') || arg == "-" {
                out.positionals.push(arg);
                continue;
            }
            let raw = arg.trim_start_matches('-');
            if let Some((name, val)) = raw.split_once('=') {
                apply_inline_flag(&mut out, name, val, value_flags, bool_flags)?;
            } else if bool_flags.contains(&raw) {
                out.bools.insert(raw.to_owned(), true);
            } else if value_flags.contains(&raw) {
                let val = iter
                    .next()
                    .ok_or_else(|| format!("flag -{raw} requires a value"))?;
                out.values.entry(raw.to_owned()).or_default().push(val);
            } else {
                return Err(format!("unknown flag: {arg}"));
            }
        }
        Ok(out)
    }
}

fn apply_inline_flag(
    out: &mut ParsedArgs,
    name: &str,
    val: &str,
    value_flags: &[&str],
    bool_flags: &[&str],
) -> Result<(), String> {
    if bool_flags.contains(&name) {
        let b = parse_flag_bool(val).ok_or_else(|| format!("invalid bool for -{name}: {val}"))?;
        out.bools.insert(name.to_owned(), b);
        return Ok(());
    }
    if value_flags.contains(&name) {
        out.values
            .entry(name.to_owned())
            .or_default()
            .push(val.to_owned());
        return Ok(());
    }
    Err(format!("unknown flag: -{name}"))
}

fn parse_flag_bool(val: &str) -> Option<bool> {
    match val.trim().to_ascii_lowercase().as_str() {
        "1" | "t" | "true" | "yes" => Some(true),
        "0" | "f" | "false" | "no" => Some(false),
        _ => None,
    }
}
