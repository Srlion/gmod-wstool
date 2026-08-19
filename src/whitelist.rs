use arc_swap::ArcSwap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
use wildmatch::WildMatch;

const RAW_URL: &str =
    "https://raw.githubusercontent.com/Facepunch/gmad/master/include/AddonWhiteList.h";

pub const STORAGE_KEY: &str = "addon_whitelist";

const FALLBACK: &[&str] = &[
    "lua/*.lua",
    "scenes/*.vcd",
    "particles/*.pcf",
    "resource/fonts/*.ttf",
    "scripts/vehicles/*.txt",
    "resource/localization/*/*.properties",
    "maps/*.bsp",
    "maps/*.lmp",
    "maps/*.nav",
    "maps/*.ain",
    "maps/thumb/*.png",
    "sound/*.wav",
    "sound/*.mp3",
    "sound/*.ogg",
    "materials/*.vmt",
    "materials/*.vtf",
    "materials/*.png",
    "materials/*.jpg",
    "materials/*.jpeg",
    "materials/colorcorrection/*.raw",
    "models/*.mdl",
    "models/*.phy",
    "models/*.ani",
    "models/*.vvd",
    "models/*.vtx",
    "!models/*.sw.vtx",
    "!models/*.360.vtx",
    "!models/*.xbox.vtx",
    "gamemodes/*/*.txt",
    "!gamemodes/*/*/*.txt",
    "gamemodes/*/*.fgd",
    "!gamemodes/*/*/*.fgd",
    "gamemodes/*/logo.png",
    "gamemodes/*/icon24.png",
    "gamemodes/*/gamemode/*.lua",
    "gamemodes/*/entities/effects/*.lua",
    "gamemodes/*/entities/weapons/*.lua",
    "gamemodes/*/entities/entities/*.lua",
    "gamemodes/*/backgrounds/*.png",
    "gamemodes/*/backgrounds/*.jpg",
    "gamemodes/*/backgrounds/*.jpeg",
    "gamemodes/*/content/models/*.mdl",
    "gamemodes/*/content/models/*.phy",
    "gamemodes/*/content/models/*.ani",
    "gamemodes/*/content/models/*.vvd",
    "gamemodes/*/content/models/*.vtx",
    "!gamemodes/*/content/models/*.sw.vtx",
    "!gamemodes/*/content/models/*.360.vtx",
    "!gamemodes/*/content/models/*.xbox.vtx",
    "gamemodes/*/content/materials/*.vmt",
    "gamemodes/*/content/materials/*.vtf",
    "gamemodes/*/content/materials/*.png",
    "gamemodes/*/content/materials/*.jpg",
    "gamemodes/*/content/materials/*.jpeg",
    "gamemodes/*/content/materials/colorcorrection/*.raw",
    "gamemodes/*/content/scenes/*.vcd",
    "gamemodes/*/content/particles/*.pcf",
    "gamemodes/*/content/resource/fonts/*.ttf",
    "gamemodes/*/content/scripts/vehicles/*.txt",
    "gamemodes/*/content/resource/localization/*/*.properties",
    "gamemodes/*/content/maps/*.bsp",
    "gamemodes/*/content/maps/*.nav",
    "gamemodes/*/content/maps/*.ain",
    "gamemodes/*/content/maps/thumb/*.png",
    "gamemodes/*/content/sound/*.wav",
    "gamemodes/*/content/sound/*.mp3",
    "gamemodes/*/content/sound/*.ogg",
    "data_static/*.txt",
    "data_static/*.dat",
    "data_static/*.json",
    "data_static/*.xml",
    "data_static/*.csv",
    "shaders/fxc/*.vcs",
];

struct Rule {
    matcher: WildMatch,
    negate: bool,
}

fn compile(patterns: &[String]) -> Vec<Rule> {
    patterns
        .iter()
        .map(|p| {
            let negate = p.starts_with('!');
            let pat = if negate { &p[1..] } else { p.as_str() };
            Rule {
                matcher: WildMatch::new(pat),
                negate,
            }
        })
        .collect()
}

static RULES: LazyLock<ArcSwap<Vec<Rule>>> =
    LazyLock::new(|| ArcSwap::from_pointee(compile(&fallback_patterns())));

static LATEST: LazyLock<Mutex<Vec<String>>> = LazyLock::new(|| Mutex::new(fallback_patterns()));

fn fallback_patterns() -> Vec<String> {
    FALLBACK.iter().map(|s| s.to_string()).collect()
}

pub fn seed(stored: Option<String>) {
    let Some(raw) = stored else { return };
    let Ok(patterns) = serde_json::from_str::<Vec<String>>(&raw) else {
        return; // corrupt or old format: keep fallback
    };
    if patterns.is_empty() {
        return;
    }
    RULES.store(Arc::new(compile(&patterns)));
    *LATEST.lock().unwrap() = patterns;
}

pub fn to_storage_string() -> String {
    let patterns = LATEST.lock().unwrap().clone();
    serde_json::to_string(&patterns).unwrap_or_default()
}

fn parse_header(src: &str) -> Option<Vec<String>> {
    let body = src
        .split_once("Wildcard[] =")
        .and_then(|(_, rest)| rest.split_once('{'))
        .map(|(_, rest)| rest)?;

    let mut out = Vec::new();
    for line in body.lines() {
        let code = line.split("//").next().unwrap_or("").trim();
        if code.starts_with("NULL") {
            break;
        }
        if let (Some(a), Some(b)) = (code.find('"'), code.rfind('"'))
            && b > a
        {
            out.push(code[a + 1..b].to_string());
        }
    }
    // the page layout probably changed; don't trust it.
    (out.len() >= 20).then_some(out)
}

pub fn refresh_blocking() -> bool {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build();
    let agent: ureq::Agent = config.into();

    let body = match agent
        .get(RAW_URL)
        .call()
        .and_then(|mut r| r.body_mut().read_to_string())
    {
        Ok(b) => b,
        Err(_) => return false,
    };

    let Some(patterns) = parse_header(&body) else {
        return false;
    };

    RULES.store(Arc::new(compile(&patterns)));
    *LATEST.lock().unwrap() = patterns;
    true
}

pub fn is_allowed(rel_path: &str) -> bool {
    let rules = RULES.load();
    let mut valid = false;
    for rule in rules.iter() {
        if rule.negate {
            if rule.matcher.matches(rel_path) {
                valid = false;
            }
        } else if !valid && rule.matcher.matches(rel_path) {
            valid = true;
        }
    }
    valid
}
