//! Estado puro de layout: qué muestra la vista central. Sin dependencias de Leptos
//! para poder probarlo con `cargo test` nativo.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CenterView {
    Agent,
    Editor,
    Diff,
}

impl CenterView {
    pub const ALL: [CenterView; 3] = [CenterView::Agent, CenterView::Editor, CenterView::Diff];

    pub fn label(self) -> &'static str {
        match self {
            CenterView::Agent => "Agente",
            CenterView::Editor => "Editor",
            CenterView::Diff => "Diff",
        }
    }

    /// Atajo de teclado documentado en docs/architecture/ui.md.
    pub fn shortcut(self) -> &'static str {
        match self {
            CenterView::Agent => "Ctrl+1",
            CenterView::Editor => "Ctrl+2",
            CenterView::Diff => "Ctrl+3",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CenterView;

    #[test]
    fn center_view_has_three_tabs_in_order() {
        let labels: Vec<_> = CenterView::ALL.iter().map(|v| v.label()).collect();
        assert_eq!(labels, ["Agente", "Editor", "Diff"]);
    }

    #[test]
    fn shortcuts_are_unique() {
        let mut shortcuts: Vec<_> = CenterView::ALL.iter().map(|v| v.shortcut()).collect();
        shortcuts.dedup();
        assert_eq!(shortcuts.len(), CenterView::ALL.len());
    }
}
