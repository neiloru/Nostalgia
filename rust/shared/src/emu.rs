//! emu.yaml validation, argument building and launch file selection (port of shared/src/emu.ts).

use crate::collate::natural_compare;
use crate::types::{Arguments, EmuConfig, EmulatorSystemConfig, OsKey, Systems};
use indexmap::IndexMap;
use regex::Regex;
use serde_yaml::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

/// JavaScript `String(value)` for YAML scalars.
fn js_string(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            if let Some(f) = n.as_f64().filter(|f| f.fract() == 0.0 && n.is_f64()) {
                format!("{}", f as i64)
            } else {
                n.to_string()
            }
        }
        Value::String(s) => s.clone(),
        Value::Sequence(items) => items.iter().map(js_string).collect::<Vec<_>>().join(","),
        Value::Mapping(_) => "[object Object]".into(),
        Value::Tagged(t) => js_string(&t.value),
    }
}

/// JavaScript truthiness.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0 && !f.is_nan()).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

fn get<'a>(map: &'a serde_yaml::Mapping, key: &str) -> Option<&'a Value> {
    map.get(Value::String(key.into())).filter(|v| !v.is_null())
}

/// Validate an already-parsed emu.yaml document. Errors are readable messages.
pub fn validate_emu_config(doc: &Value) -> Result<EmuConfig, String> {
    let d = match doc {
        Value::Mapping(m) => m,
        _ => return Err("emu.yaml must be a mapping".into()),
    };
    let systems = match get(d, "systems") {
        Some(Value::Mapping(m)) => m,
        Some(Value::Sequence(_)) => &serde_yaml::Mapping::new(),
        _ => return Err("emu.yaml: \"systems\" with at least \"windows\" or \"linux\" is required".into()),
    };
    let mut out = EmuConfig::default();
    if let Some(v) = get(d, "name") {
        out.name = Some(js_string(v));
    }
    if let Some(v) = get(d, "description") {
        out.description = Some(js_string(v));
    }
    if let Some(v) = get(d, "version") {
        out.version = Some(js_string(v));
    }
    if let Some(v) = get(d, "extensions") {
        let Value::Sequence(list) = v else { return Err("emu.yaml: \"extensions\" must be a list".into()) };
        out.extensions = Some(
            list.iter()
                .map(|e| {
                    let s = js_string(e);
                    s.strip_prefix('.').unwrap_or(&s).to_lowercase()
                })
                .collect(),
        );
    }
    let mut sys_out = Systems::default();
    for os in [OsKey::Windows, OsKey::Linux] {
        let Some(s) = get(systems, os.as_str()) else { continue };
        let s = match s {
            Value::Mapping(m) => m,
            Value::Sequence(_) => &serde_yaml::Mapping::new(),
            _ => return Err(format!("emu.yaml: systems.{} must be a mapping", os.as_str())),
        };
        let exe = s.get(Value::String("executable".into()));
        if !exe.map(truthy).unwrap_or(false) {
            return Err(format!("emu.yaml: systems.{}.executable is required", os.as_str()));
        }
        let mut sys = EmulatorSystemConfig {
            executable: js_string(exe.unwrap()),
            arguments: None,
            working_directory: None,
            saves: None,
            save_folder: None,
            env: None,
        };
        if let Some(a) = get(s, "arguments") {
            sys.arguments = Some(match a {
                Value::Sequence(items) => Arguments::List(items.iter().map(js_string).collect()),
                other => Arguments::Line(js_string(other)),
            });
        }
        if let Some(w) = get(s, "workingDirectory") {
            sys.working_directory = Some(js_string(w));
        }
        let mut saves: Vec<String> = Vec::new();
        match get(s, "saves") {
            Some(Value::Sequence(items)) => saves.extend(items.iter().map(js_string)),
            Some(Value::String(one)) => saves.push(one.clone()),
            _ => {}
        }
        if let Some(f) = get(s, "saveFolder") {
            saves.push(js_string(f));
        }
        if !saves.is_empty() {
            let mut seen = Vec::new();
            for x in saves {
                if !seen.contains(&x) {
                    seen.push(x);
                }
            }
            sys.saves = Some(seen);
        }
        if let Some(Value::Mapping(env)) = get(s, "env") {
            let mut m = IndexMap::new();
            for (k, v) in env {
                m.insert(js_string(k), js_string(v));
            }
            sys.env = Some(m);
        }
        match os {
            OsKey::Windows => sys_out.windows = Some(sys),
            OsKey::Linux => sys_out.linux = Some(sys),
        }
    }
    if sys_out.windows.is_none() && sys_out.linux.is_none() {
        return Err("emu.yaml: define systems.windows and/or systems.linux".into());
    }
    out.systems = sys_out;
    Ok(out)
}

/// Parse and validate emu.yaml text.
pub fn parse_emu_yaml(text: &str) -> Result<EmuConfig, String> {
    let doc: Value = serde_yaml::from_str(text).map_err(|e| e.to_string())?;
    validate_emu_config(&doc)
}

/// Split a command line string into arguments, honouring double and single quotes.
pub fn split_args(input: &str) -> Vec<String> {
    let chars: Vec<char> = input.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut has = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else if c == '\\' && q == '"' && chars.get(i + 1) == Some(&'"') {
                cur.push('"');
                i += 1;
            } else {
                cur.push(c);
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
            has = true;
        } else if c.is_whitespace() {
            if has || !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            has = false;
        } else {
            cur.push(c);
            has = true;
        }
        i += 1;
    }
    if has || !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub struct ExpandContext<'a> {
    /// `{rom}`, `{emulatorDir}`, ...
    pub vars: &'a HashMap<String, String>,
    pub env: &'a HashMap<String, String>,
    pub home: &'a str,
    pub os: OsKey,
}

static VAR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{([a-zA-Z]+)\}").unwrap());
static WIN_ENV_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"%([A-Za-z_][A-Za-z0-9_]*)%").unwrap());
static SH_ENV_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}|\$([A-Za-z_][A-Za-z0-9_]*)").unwrap());

/// Expand `{var}` placeholders, `%ENV%` (windows style), `$ENV` / `${ENV}` and a leading `~`.
/// Unknown placeholders are left untouched.
pub fn expand_string(value: &str, ctx: &ExpandContext) -> String {
    let env_get = |k: &str| -> Option<String> {
        if ctx.os == OsKey::Windows {
            ctx.env.iter().find(|(e, _)| e.eq_ignore_ascii_case(k)).map(|(_, v)| v.clone())
        } else {
            ctx.env.get(k).cloned()
        }
    };
    let s = VAR_RE.replace_all(value, |c: &regex::Captures| {
        let k = &c[1];
        if let Some(v) = ctx.vars.get(k) {
            v.clone()
        } else if k == "home" {
            ctx.home.to_string()
        } else {
            c[0].to_string()
        }
    });
    let s = WIN_ENV_RE.replace_all(&s, |c: &regex::Captures| {
        let k = &c[1];
        env_get(k)
            .or_else(|| {
                let u = k.to_uppercase();
                (u == "USERPROFILE" || u == "HOME").then(|| ctx.home.to_string())
            })
            .unwrap_or_else(|| c[0].to_string())
    });
    let s = SH_ENV_RE.replace_all(&s, |c: &regex::Captures| {
        let k = c.get(1).or_else(|| c.get(2)).unwrap().as_str();
        env_get(k).or_else(|| (k == "HOME").then(|| ctx.home.to_string())).unwrap_or_else(|| c[0].to_string())
    });
    let s = s.into_owned();
    if s == "~" || s.starts_with("~/") || s.starts_with("~\\") {
        format!("{}{}", ctx.home, &s[1..])
    } else {
        s
    }
}

pub fn build_arguments(sys: &EmulatorSystemConfig, ctx: &ExpandContext) -> Vec<String> {
    // Split before expanding so paths with spaces stay one argument.
    let parts = match &sys.arguments {
        None => vec!["{rom}".to_string()],
        Some(Arguments::List(l)) => l.clone(),
        Some(Arguments::Line(s)) => split_args(s),
    };
    parts.iter().map(|p| expand_string(p, ctx)).collect()
}

static DISC_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)[(\[\s_-](?:disc|disk|cd)\s*[-_ ]?(\d+|[a-z])\b").unwrap());

pub fn disc_label(file: &str) -> String {
    let base = file.rsplit('/').next().unwrap_or(file);
    match DISC_RE.captures(base) {
        Some(m) => format!("Disc {}", m[1].to_uppercase()),
        None => base.to_string(),
    }
}

fn base_name(f: &str) -> &str {
    f.rsplit('/').next().unwrap_or(f)
}

/// Pick launchable files from a game's file list.
/// Returns all files having the highest-priority extension found (e.g. all `.cue` of a multi disc game).
/// Files inside the `igdb/` asset folder are ignored.
pub fn launch_candidates(files: &[String], extensions: &[String]) -> Vec<String> {
    let usable: Vec<&String> = files.iter().filter(|f| !f.starts_with("igdb/") && !base_name(f).starts_with('.')).collect();
    let sorted = |mut v: Vec<String>| {
        v.sort_by(|a, b| natural_compare(a, b));
        v
    };
    for ext in extensions {
        let suffix = format!(".{}", ext.to_lowercase());
        let hit: Vec<String> = usable.iter().filter(|f| f.to_lowercase().ends_with(&suffix)).map(|f| f.to_string()).collect();
        if !hit.is_empty() {
            return sorted(hit);
        }
    }
    sorted(usable.into_iter().cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(vars: &'a HashMap<String, String>, env: &'a HashMap<String, String>, os: OsKey) -> ExpandContext<'a> {
        ExpandContext { vars, env, home: "/home/x", os }
    }
    fn map(p: &[(&str, &str)]) -> HashMap<String, String> {
        p.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn split_args_honours_quotes() {
        assert_eq!(split_args(r#"-nogui -fullscreen "{rom}""#), vec!["-nogui", "-fullscreen", "{rom}"]);
        assert_eq!(split_args(r#"-a 'b c' "" d"#), vec!["-a", "b c", "", "d"]);
    }

    #[test]
    fn expand_string_handles_placeholders_and_env() {
        let vars = map(&[("rom", "C:/g/a b.iso"), ("emulatorDir", "/emu")]);
        let env = map(&[("USERPROFILE", "C:/Users/x"), ("HOME", "/home/x")]);
        assert_eq!(expand_string("%userprofile%/Documents", &ctx(&vars, &env, OsKey::Windows)), "C:/Users/x/Documents");
        assert_eq!(expand_string("{emulatorDir}/memcards", &ctx(&vars, &env, OsKey::Windows)), "/emu/memcards");
        assert_eq!(expand_string("~/saves", &ctx(&vars, &env, OsKey::Linux)), "/home/x/saves");
        assert_eq!(expand_string("${HOME}/.config", &ctx(&vars, &env, OsKey::Linux)), "/home/x/.config");
        assert_eq!(expand_string("{unknown}", &ctx(&vars, &env, OsKey::Windows)), "{unknown}");
    }

    #[test]
    fn build_arguments_keeps_paths_with_spaces() {
        let vars = map(&[("rom", "C:/My Games/ff x.iso")]);
        let env = HashMap::new();
        let c = ExpandContext { vars: &vars, env: &env, home: "/h", os: OsKey::Windows };
        let sys = |a: Option<Arguments>| EmulatorSystemConfig {
            executable: "x".into(),
            arguments: a,
            working_directory: None,
            saves: None,
            save_folder: None,
            env: None,
        };
        assert_eq!(
            build_arguments(&sys(Some(Arguments::Line(r#"-nogui -fullscreen "{rom}""#.into()))), &c),
            vec!["-nogui", "-fullscreen", "C:/My Games/ff x.iso"]
        );
        assert_eq!(build_arguments(&sys(None), &c), vec!["C:/My Games/ff x.iso"]);
    }

    #[test]
    fn launch_candidates_prefers_cue_and_sorts_naturally() {
        let files: Vec<String> = ["igdb/cover.jpg", "Game (Disc 10).cue", "Game (Disc 2).bin", "Game (Disc 2).cue", "Game (Disc 1).cue", "Game (Disc 1).bin"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let exts: Vec<String> = ["m3u", "cue", "bin"].iter().map(|s| s.to_string()).collect();
        assert_eq!(launch_candidates(&files, &exts), vec!["Game (Disc 1).cue", "Game (Disc 2).cue", "Game (Disc 10).cue"]);
        assert_eq!(disc_label("Final Fantasy VIII (USA) (Disc 3).cue"), "Disc 3");
    }

    #[test]
    fn validate_emu_config_accepts_save_folder_and_rejects_broken_files() {
        let c = parse_emu_yaml("name: PCSX2\nsystems:\n  windows:\n    executable: pcsx2-qt.exe\n    saveFolder: '%USERPROFILE%/Documents/PCSX2/memcards'\n").unwrap();
        assert_eq!(c.systems.windows.unwrap().saves, Some(vec!["%USERPROFILE%/Documents/PCSX2/memcards".to_string()]));
        assert!(parse_emu_yaml("name: x").is_err());
        assert!(parse_emu_yaml("systems:\n  linux: {}\n").is_err());
        // JSON written by the Node.js server: systems first, then the rest.
        let c = parse_emu_yaml("name: TestEmu\nversion: 1.5\nsystems:\n  windows:\n    executable: emu.exe\n").unwrap();
        assert_eq!(serde_json::to_string(&c).unwrap(), r#"{"systems":{"windows":{"executable":"emu.exe"}},"name":"TestEmu","version":"1.5"}"#);
    }
}
