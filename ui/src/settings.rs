//! Preferencias de apariencia (tipografías y tamaños). Lógica pura; se guardan en
//! el `localStorage` de la ventana (`bridge::prefs`) porque son de la UI, no del proyecto.

/// (etiqueta, pila CSS)
pub const UI_FONTS: &[(&str, &str)] = &[
    ("Sistema", r#"system-ui, "Segoe UI", sans-serif"#),
    ("Segoe UI", r#""Segoe UI", system-ui, sans-serif"#),
    ("Inter", r#"Inter, system-ui, sans-serif"#),
    ("Roboto", r#"Roboto, system-ui, sans-serif"#),
    ("Arial", r#"Arial, Helvetica, sans-serif"#),
    ("Verdana", r#"Verdana, Geneva, sans-serif"#),
];

pub const MONO_FONTS: &[(&str, &str)] = &[
    (
        "Cascadia Mono",
        r#""Cascadia Mono", Consolas, "DejaVu Sans Mono", monospace"#,
    ),
    (
        "Cascadia Code",
        r#""Cascadia Code", "Cascadia Mono", Consolas, monospace"#,
    ),
    ("Consolas", r#"Consolas, "Cascadia Mono", monospace"#),
    (
        "JetBrains Mono",
        r#""JetBrains Mono", "Cascadia Mono", Consolas, monospace"#,
    ),
    (
        "Fira Code",
        r#""Fira Code", "Cascadia Mono", Consolas, monospace"#,
    ),
    ("Courier New", r#""Courier New", Courier, monospace"#),
];

/// Agente que se lanza al crear una feature: (id del proveedor, etiqueta).
/// El id vacío significa "ninguno".
pub const INITIAL_AGENTS: &[(&str, &str)] = &[
    ("", "Ninguno"),
    ("claude", "Claude Code"),
    ("codex", "Codex"),
    ("opencode", "OpenCode"),
];

pub const SIZE_MIN: u8 = 10;
pub const SIZE_MAX: u8 = 24;
pub const STORAGE_KEY: &str = "gryphus.settings";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Etiqueta de [`UI_FONTS`].
    pub ui_font: String,
    pub ui_size: u8,
    /// Etiqueta de [`MONO_FONTS`] (editor, terminal y diff).
    pub mono_font: String,
    pub mono_size: u8,
    /// Id de [`INITIAL_AGENTS`]; vacío = no lanzar agente al crear una feature.
    pub initial_agent: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ui_font: UI_FONTS[0].0.to_owned(),
            ui_size: 13,
            mono_font: MONO_FONTS[0].0.to_owned(),
            mono_size: 13,
            initial_agent: String::new(),
        }
    }
}

fn stack<'a>(fonts: &'a [(&str, &'a str)], label: &str) -> &'a str {
    fonts
        .iter()
        .find(|(l, _)| *l == label)
        .unwrap_or(&fonts[0])
        .1
}

pub fn clamp_size(size: u8) -> u8 {
    size.clamp(SIZE_MIN, SIZE_MAX)
}

impl Settings {
    /// Variables CSS de `:root` (ver ui/style.css).
    pub fn css_vars(&self) -> [(&'static str, String); 4] {
        [
            ("--ui-font", stack(UI_FONTS, &self.ui_font).to_owned()),
            ("--ui-size", format!("{}px", self.ui_size)),
            ("--mono", stack(MONO_FONTS, &self.mono_font).to_owned()),
            ("--mono-size", format!("{}px", self.mono_size)),
        ]
    }

    /// Formato `clave=valor` por línea; tolera claves desconocidas y valores inválidos.
    pub fn serialize(&self) -> String {
        format!(
            "ui_font={}\nui_size={}\nmono_font={}\nmono_size={}\ninitial_agent={}\n",
            self.ui_font, self.ui_size, self.mono_font, self.mono_size, self.initial_agent
        )
    }

    /// Proveedor a lanzar al crear una feature, si hay uno elegido.
    pub fn initial_agent(&self) -> Option<&str> {
        Some(self.initial_agent.as_str()).filter(|id| !id.is_empty())
    }

    pub fn parse(text: &str) -> Self {
        let mut s = Self::default();
        let known = |fonts: &[(&str, &str)], v: &str| fonts.iter().any(|(l, _)| *l == v);
        for (key, value) in text.lines().filter_map(|l| l.split_once('=')) {
            let value = value.trim();
            match key.trim() {
                "ui_font" if known(UI_FONTS, value) => s.ui_font = value.to_owned(),
                "mono_font" if known(MONO_FONTS, value) => s.mono_font = value.to_owned(),
                "initial_agent" if INITIAL_AGENTS.iter().any(|(id, _)| *id == value) => {
                    s.initial_agent = value.to_owned()
                }
                "ui_size" => {
                    if let Ok(n) = value.parse() {
                        s.ui_size = clamp_size(n);
                    }
                }
                "mono_size" => {
                    if let Ok(n) = value.parse() {
                        s.mono_size = clamp_size(n);
                    }
                }
                _ => {}
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let s = Settings {
            ui_font: "Arial".into(),
            ui_size: 15,
            mono_font: "Fira Code".into(),
            mono_size: 12,
            initial_agent: "codex".into(),
        };
        assert_eq!(Settings::parse(&s.serialize()), s);
        assert_eq!(s.initial_agent(), Some("codex"));
        assert_eq!(Settings::default().initial_agent(), None);
    }

    #[test]
    fn invalid_values_fall_back_to_defaults() {
        let s =
            Settings::parse("ui_font=Nope\nui_size=200\nmono_size=x\ninitial_agent=vim\nbasura");
        assert_eq!(s.initial_agent, "");
        assert_eq!(s.ui_font, Settings::default().ui_font);
        assert_eq!(s.ui_size, SIZE_MAX);
        assert_eq!(s.mono_size, Settings::default().mono_size);
        assert_eq!(Settings::parse(""), Settings::default());
    }

    #[test]
    fn css_vars_use_font_stacks() {
        let vars = Settings::default().css_vars();
        assert_eq!(vars[1], ("--ui-size", "13px".to_owned()));
        assert!(vars[2].1.contains("Cascadia Mono"));
    }
}
