//! Génome : seed + paramètres de style + surcharges locales (§ 5.9).
//!
//! Format texte compact : `RMP1.<style>.<seed base32>.<CBOR base64url>`.

use crate::seed::{from_base32, to_base32_compact};
use crate::style::{Family, StyleParams};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Version sémantique du générateur. Tout changement de comportement d'une
/// règle l'incrémente (seeds dorées).
pub const GENERATOR_VERSION: &str = "0.6.2";

/// Chemin d'un nœud depuis la racine (ex. `0/2/1`).
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodePath(pub Vec<u16>);

impl NodePath {
    pub fn root() -> Self {
        NodePath(vec![0])
    }
    pub fn child(&self, i: usize) -> Self {
        let mut v = self.0.clone();
        v.push(i.min(u16::MAX as usize) as u16);
        NodePath(v)
    }
    pub fn parse(s: &str) -> Option<Self> {
        let v: Option<Vec<u16>> = s.split('/').map(|p| p.trim().parse().ok()).collect();
        v.filter(|v| !v.is_empty()).map(NodePath)
    }
    pub fn depth(&self) -> usize {
        self.0.len().saturating_sub(1)
    }
    pub fn starts_with(&self, o: &NodePath) -> bool {
        self.0.starts_with(&o.0)
    }
}

impl fmt::Display for NodePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s: Vec<String> = self.0.iter().map(|i| i.to_string()).collect();
        write!(f, "{}", s.join("/"))
    }
}

/// Paramètres d'une règle (clés ordonnées pour le déterminisme).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Params(pub BTreeMap<String, f64>);

impl Params {
    pub fn new() -> Self {
        Params(BTreeMap::new())
    }
    pub fn with(mut self, k: &str, v: f64) -> Self {
        self.0.insert(k.to_string(), v);
        self
    }
    pub fn set(&mut self, k: &str, v: f64) {
        self.0.insert(k.to_string(), v);
    }
    pub fn get(&self, k: &str, default: f64) -> f64 {
        self.0.get(k).copied().unwrap_or(default)
    }
    pub fn get_usize(&self, k: &str, default: usize) -> usize {
        self.0
            .get(k)
            .map(|v| v.max(0.0) as usize)
            .unwrap_or(default)
    }
}

/// Règle verrouillée sur un nœud.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lock {
    pub rule: String,
    pub params: Params,
}

/// Surcharge locale : sel de relance et verrou.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NodeOverride {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub salt: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock: Option<Lock>,
    /// Règle imposée (paramètres tirés normalement), EF-34.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_rule: Option<String>,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

/// Description complète et compacte d'une variante.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Genome {
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(with = "seed_serde")]
    pub seed: u64,
    pub style: StyleParams,
    #[serde(default)]
    pub overrides: BTreeMap<String, NodeOverride>,
}

fn default_version() -> String {
    GENERATOR_VERSION.to_string()
}

mod seed_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&crate::seed::to_base32(*v))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum V {
            S(String),
            N(u64),
        }
        Ok(match V::deserialize(d)? {
            V::S(s) => crate::seed::parse_seed(&s),
            V::N(n) => n,
        })
    }
}

impl Genome {
    pub fn new(seed: u64, style: StyleParams) -> Self {
        Genome {
            version: GENERATOR_VERSION.to_string(),
            seed,
            style,
            overrides: BTreeMap::new(),
        }
    }
    pub fn override_for(&self, p: &NodePath) -> Option<&NodeOverride> {
        self.overrides.get(&p.to_string())
    }
    /// Relance locale : sel += 1 sur le nœud.
    pub fn reroll(&mut self, p: &NodePath) {
        let e = self.overrides.entry(p.to_string()).or_default();
        e.salt = e.salt.wrapping_add(1);
    }
    pub fn lock(&mut self, p: &NodePath, rule: &str, params: Params) {
        let e = self.overrides.entry(p.to_string()).or_default();
        e.lock = Some(Lock {
            rule: rule.to_string(),
            params,
        });
    }
    pub fn force_rule(&mut self, p: &NodePath, rule: Option<&str>) {
        let e = self.overrides.entry(p.to_string()).or_default();
        e.force_rule = rule.map(|r| r.to_string());
        e.lock = None;
    }
    /// Retire les surcharges vides.
    pub fn prune(&mut self) {
        self.overrides
            .retain(|_, o| o.salt != 0 || o.lock.is_some() || o.force_rule.is_some());
    }
    pub fn unlock(&mut self, p: &NodePath) {
        if let Some(e) = self.overrides.get_mut(&p.to_string()) {
            e.lock = None;
        }
    }

    /// Format compact `RMP1.<style>.<seed>.<charge utile>`.
    pub fn to_compact(&self) -> String {
        #[derive(Serialize)]
        struct Payload<'a> {
            v: &'a str,
            s: &'a StyleParams,
            o: &'a BTreeMap<String, NodeOverride>,
        }
        let mut buf = Vec::new();
        let p = Payload {
            v: &self.version,
            s: &self.style,
            o: &self.overrides,
        };
        // L'écriture dans un Vec<u8> ne peut pas échouer.
        if ciborium::into_writer(&p, &mut buf).is_err() {
            buf.clear();
        }
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&buf);
        format!(
            "RMP1.{}.{}.{}",
            self.style.family.key(),
            to_base32_compact(self.seed),
            b64
        )
    }

    pub fn from_compact(s: &str) -> Result<Genome, String> {
        #[derive(Deserialize)]
        struct Payload {
            v: String,
            s: StyleParams,
            o: BTreeMap<String, NodeOverride>,
        }
        let parts: Vec<&str> = s.trim().splitn(4, '.').collect();
        if parts.len() < 3 || parts[0] != "RMP1" {
            return Err("génome invalide : préfixe RMP1 attendu".into());
        }
        let family = Family::parse(parts[1]).ok_or("style inconnu")?;
        let seed = from_base32(parts[2]).ok_or("seed invalide")?;
        if parts.len() == 3 || parts[3].is_empty() {
            return Ok(Genome::new(seed, StyleParams::for_family(family)));
        }
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parts[3])
            .map_err(|e| format!("base64 : {e}"))?;
        let p: Payload = ciborium::from_reader(&bytes[..]).map_err(|e| format!("CBOR : {e}"))?;
        Ok(Genome {
            version: p.v,
            seed,
            style: p.s,
            overrides: p.o,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_roundtrip() {
        let mut g = Genome::new(
            0x1234_5678_9ABC_DEF0,
            StyleParams::for_family(Family::Flamboyant),
        );
        g.reroll(&NodePath(vec![0, 2]));
        g.lock(
            &NodePath(vec![0, 0]),
            "R04",
            Params::new().with("n", 4.0).with("k", 1.2),
        );
        let s = g.to_compact();
        assert!(s.starts_with("RMP1.flamboyant."));
        let back = Genome::from_compact(&s).expect("décodage");
        assert_eq!(back, g);
    }

    #[test]
    fn json_roundtrip() {
        let g = Genome::new(42, StyleParams::default());
        let j = serde_json::to_string(&g).unwrap();
        let back: Genome = serde_json::from_str(&j).unwrap();
        assert_eq!(back, g);
    }
}
