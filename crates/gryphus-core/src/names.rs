//! Nombres validados: [`FeatureName`] y [`BranchName`].
//!
//! Validar en el constructor garantiza que un valor de estos tipos siempre es válido:
//! el resto del sistema no necesita re-validar.

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    #[error("el nombre está vacío")]
    Empty,
    #[error("el nombre supera {max} caracteres")]
    TooLong { max: usize },
    #[error("carácter no permitido: {0:?}")]
    InvalidChar(char),
    #[error("formato no permitido: {0}")]
    InvalidFormat(&'static str),
}

/// Nombre de una feature. Se usa como nombre de carpeta del worktree y como parte
/// de la branch (`feat/<nombre>`), por eso es estricto:
/// ASCII alfanumérico, `-`, `_` y `.`; empieza por letra o número; máximo 64.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct FeatureName(String);

impl FeatureName {
    pub const MAX_LEN: usize = 64;

    pub fn parse(raw: &str) -> Result<Self, NameError> {
        let s = raw.trim();
        if s.is_empty() {
            return Err(NameError::Empty);
        }
        if s.len() > Self::MAX_LEN {
            return Err(NameError::TooLong { max: Self::MAX_LEN });
        }
        if let Some(c) = s
            .chars()
            .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
        {
            return Err(NameError::InvalidChar(c));
        }
        if !s.starts_with(|c: char| c.is_ascii_alphanumeric()) {
            return Err(NameError::InvalidFormat("debe empezar por letra o número"));
        }
        if s.ends_with('.') || s.contains("..") {
            return Err(NameError::InvalidFormat(
                "no puede terminar en '.' ni contener '..'",
            ));
        }
        let lower = s.to_ascii_lowercase();
        if lower.ends_with(".lock") {
            return Err(NameError::InvalidFormat("no puede terminar en '.lock'"));
        }
        if is_windows_reserved(&lower) {
            return Err(NameError::InvalidFormat(
                "es un nombre reservado de Windows (con, nul, com1…)",
            ));
        }
        Ok(Self(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Clave de unicidad: insensible a mayúsculas (Windows y macOS no distinguen).
    pub(crate) fn key(&self) -> String {
        self.0.to_ascii_lowercase()
    }
}

fn is_windows_reserved(lower: &str) -> bool {
    let stem = lower.split('.').next().unwrap_or(lower);
    matches!(stem, "con" | "prn" | "aux" | "nul")
        || ((stem.starts_with("com") || stem.starts_with("lpt"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit())
}

/// Nombre de branch git válido según las reglas de `git check-ref-format`
/// (subconjunto conservador).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct BranchName(String);

impl BranchName {
    pub const MAX_LEN: usize = 255;

    pub fn parse(s: &str) -> Result<Self, NameError> {
        if s.is_empty() {
            return Err(NameError::Empty);
        }
        if s.len() > Self::MAX_LEN {
            return Err(NameError::TooLong { max: Self::MAX_LEN });
        }
        if let Some(c) = s.chars().find(|c| {
            c.is_ascii_control() || matches!(c, ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\')
        }) {
            return Err(NameError::InvalidChar(c));
        }
        if s == "@" || s.starts_with('-') || s.starts_with('/') {
            return Err(NameError::InvalidFormat(
                "no puede ser '@' ni empezar por '-' o '/'",
            ));
        }
        if s.ends_with('/') || s.ends_with('.') {
            return Err(NameError::InvalidFormat("no puede terminar en '/' o '.'"));
        }
        if s.contains("..") || s.contains("//") || s.contains("@{") {
            return Err(NameError::InvalidFormat(
                "no puede contener '..', '//' ni '@{'",
            ));
        }
        if s.split('/')
            .any(|part| part.starts_with('.') || part.ends_with(".lock"))
        {
            return Err(NameError::InvalidFormat(
                "ningún tramo puede empezar por '.' ni terminar en '.lock'",
            ));
        }
        Ok(Self(s.to_owned()))
    }

    /// Branch por defecto para una feature: `feat/<nombre>`.
    /// Siempre es válida porque `FeatureName` es más estricto que `BranchName`.
    pub fn for_feature(name: &FeatureName) -> Self {
        Self(format!("feat/{}", name.as_str()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Nombre visible de un agente. Solo es una etiqueta (no toca disco ni git):
/// cualquier texto imprimible, recortado, de 1 a 40 caracteres.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AgentName(String);

impl AgentName {
    pub const MAX_LEN: usize = 40;

    /// Nombres por defecto para agentes nuevos.
    pub const DEFAULTS: &'static [&'static str] = &[
        "Atlas",
        "Nova",
        "Orion",
        "Lyra",
        "Vega",
        "Sirius",
        "Luna",
        "Iris",
        "Echo",
        "Juno",
        "Zephyr",
        "Aurora",
        "Rigel",
        "Cosmo",
        "Ada",
        "Turing",
        "Hopper",
        "Kepler",
        "Tesla",
        "Curie",
        "Darwin",
        "Euler",
        "Gauss",
        "Newton",
        "Pascal",
        "Ramanujan",
        "Noether",
        "Hypatia",
        "Fenix",
        "Draco",
        "Pegaso",
        "Quimera",
        "Helios",
        "Selene",
        "Gaia",
        "Titan",
        "Polaris",
        "Andrómeda",
        "Casiopea",
        "Altair",
    ];

    pub fn parse(raw: &str) -> Result<Self, NameError> {
        let s = raw.trim();
        if s.is_empty() {
            return Err(NameError::Empty);
        }
        if s.chars().count() > Self::MAX_LEN {
            return Err(NameError::TooLong { max: Self::MAX_LEN });
        }
        if let Some(c) = s.chars().find(|c| c.is_control()) {
            return Err(NameError::InvalidChar(c));
        }
        Ok(Self(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Nombre por defecto "aleatorio" (determinista según `seed`) que no esté en `taken`.
    /// Si todos están usados, añade un número: `Nova 2`.
    pub fn pick_default(seed: u64, taken: impl Fn(&str) -> bool) -> Self {
        let n = Self::DEFAULTS.len();
        // Mezcla de Fibonacci: ids consecutivos dan nombres que no siguen el orden de la lista.
        let start = (seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32) as usize % n;
        for round in 1.. {
            for i in 0..n {
                let base = Self::DEFAULTS[(start + i) % n];
                let name = if round == 1 {
                    base.to_owned()
                } else {
                    format!("{base} {round}")
                };
                if !taken(&name) {
                    return Self(name);
                }
            }
        }
        unreachable!("el bucle siempre encuentra un nombre libre")
    }
}

macro_rules! string_newtype_impls {
    ($t:ty) => {
        impl TryFrom<String> for $t {
            type Error = NameError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(&value)
            }
        }
        impl From<$t> for String {
            fn from(value: $t) -> Self {
                value.0
            }
        }
        impl fmt::Display for $t {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}
string_newtype_impls!(FeatureName);
string_newtype_impls!(BranchName);
string_newtype_impls!(AgentName);

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn feature_name_accepts_typical_names() {
        for ok in ["login", "pagos-v2", "Fix_123", "api.v1", "  trimmed  "] {
            assert!(FeatureName::parse(ok).is_ok(), "{ok} debería ser válido");
        }
        assert_eq!(
            FeatureName::parse("  trimmed ").unwrap().as_str(),
            "trimmed"
        );
    }

    #[test]
    fn feature_name_rejects_invalid_names() {
        let cases = [
            ("", NameError::Empty),
            ("   ", NameError::Empty),
            ("con espacio", NameError::InvalidChar(' ')),
            ("a/b", NameError::InvalidChar('/')),
            ("ñandú", NameError::InvalidChar('ñ')),
        ];
        for (input, expected) in cases {
            assert_eq!(FeatureName::parse(input), Err(expected), "{input:?}");
        }
        for bad in [
            "-x", ".x", "x.", "a..b", "x.lock", "CON", "nul.txt", "com1", "LPT9",
        ] {
            assert!(
                matches!(FeatureName::parse(bad), Err(NameError::InvalidFormat(_))),
                "{bad} debería ser inválido"
            );
        }
        assert_eq!(
            FeatureName::parse(&"a".repeat(65)),
            Err(NameError::TooLong { max: 64 })
        );
        assert!(
            FeatureName::parse("console").is_ok(),
            "solo el nombre exacto está reservado"
        );
    }

    #[test]
    fn branch_name_follows_git_rules() {
        for ok in ["main", "feat/login", "release-1.2", "user/a_b"] {
            assert!(BranchName::parse(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "@",
            "-x",
            "/x",
            "x/",
            "x.",
            "a..b",
            "a//b",
            "a@{b",
            "a b",
            "a~b",
            "a^b",
            "a:b",
            "a?b",
            "a*b",
            "a[b",
            "a\\b",
            "x.lock",
            "a/.hidden",
            "a/b.lock/c",
            "a\u{7f}b",
        ] {
            assert!(
                BranchName::parse(bad).is_err(),
                "{bad:?} debería ser inválido"
            );
        }
    }

    #[test]
    fn names_round_trip_through_serde_and_reject_invalid_json() {
        let name = FeatureName::parse("login").unwrap();
        let json = serde_json::to_string(&name).unwrap();
        assert_eq!(json, "\"login\"");
        assert_eq!(serde_json::from_str::<FeatureName>(&json).unwrap(), name);
        assert!(serde_json::from_str::<FeatureName>("\"a b\"").is_err());
        assert!(serde_json::from_str::<BranchName>("\"a..b\"").is_err());
    }

    #[test]
    fn agent_name_is_a_free_label() {
        assert_eq!(
            AgentName::parse("  Mi agente ñ ").unwrap().as_str(),
            "Mi agente ñ"
        );
        assert_eq!(AgentName::parse("  "), Err(NameError::Empty));
        assert_eq!(AgentName::parse("a\nb"), Err(NameError::InvalidChar('\n')));
        assert_eq!(
            AgentName::parse(&"x".repeat(41)),
            Err(NameError::TooLong { max: 40 })
        );
    }

    #[test]
    fn default_agent_names_avoid_taken_ones() {
        let first = AgentName::pick_default(7, |_| false);
        assert!(AgentName::DEFAULTS.contains(&first.as_str()));
        let other = AgentName::pick_default(7, |n| n == first.as_str());
        assert_ne!(other, first);
        let numbered = AgentName::pick_default(7, |n| AgentName::DEFAULTS.contains(&n));
        assert!(numbered.as_str().ends_with(" 2"), "{numbered:?}");
        assert!(AgentName::parse(numbered.as_str()).is_ok());
    }

    proptest! {
        /// Toda FeatureName válida produce una branch por defecto válida.
        #[test]
        fn every_feature_name_yields_a_valid_branch(raw in "[A-Za-z0-9][A-Za-z0-9._-]{0,63}") {
            if let Ok(name) = FeatureName::parse(&raw) {
                let branch = BranchName::for_feature(&name);
                prop_assert_eq!(BranchName::parse(branch.as_str()), Ok(branch.clone()));
            }
        }

        /// Ninguna entrada arbitraria hace entrar en pánico al parser.
        #[test]
        fn parsers_never_panic(raw in "\\PC{0,80}") {
            let _ = FeatureName::parse(&raw);
            let _ = BranchName::parse(&raw);
        }
    }
}
