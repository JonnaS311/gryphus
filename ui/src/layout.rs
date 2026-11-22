//! Estado puro de layout: qué muestra la vista central y dónde y con qué ancho se
//! acoplan los paneles laterales. Sin dependencias de Leptos
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

    /// Atajo de teclado documentado en docs/architecture/ui.md (⌘ en macOS).
    pub fn shortcut(self, mac: bool) -> &'static str {
        match (self, mac) {
            (CenterView::Agent, false) => "Ctrl+1",
            (CenterView::Editor, false) => "Ctrl+2",
            (CenterView::Diff, false) => "Ctrl+3",
            (CenterView::Agent, true) => "⌘1",
            (CenterView::Editor, true) => "⌘2",
            (CenterView::Diff, true) => "⌘3",
        }
    }

    fn key(self) -> &'static str {
        match self {
            CenterView::Agent => "agent",
            CenterView::Editor => "editor",
            CenterView::Diff => "diff",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        CenterView::ALL.into_iter().find(|v| v.key() == key)
    }
}

/// Una columna de la vista central: sus pestañas y la que se ve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub tabs: Vec<CenterView>,
    pub active: CenterView,
}

/// Dónde se suelta una pestaña arrastrada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropTarget {
    /// Como pestaña de la columna (zona superior o centro de la columna).
    Into(usize),
    /// En una columna nueva a la izquierda o a la derecha de la columna.
    Split { column: usize, after: bool },
}

pub const CENTER_STORAGE_KEY: &str = "gryphus.center";

/// Reparto de Agente / Editor / Diff en columnas lado a lado. Invariantes: cada
/// vista está en exactamente una columna, no hay columnas vacías y la pestaña
/// activa de cada columna es una de las suyas. Las vistas nunca se remontan:
/// esto solo decide en qué columna se pintan y cuál se ve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CenterLayout {
    pub columns: Vec<Column>,
}

impl Default for CenterLayout {
    fn default() -> Self {
        Self {
            columns: vec![Column {
                tabs: CenterView::ALL.to_vec(),
                active: CenterView::Agent,
            }],
        }
    }
}

impl CenterLayout {
    pub fn column_of(&self, view: CenterView) -> usize {
        self.columns
            .iter()
            .position(|c| c.tabs.contains(&view))
            .unwrap_or(0)
    }

    /// La vista se está pintando (es la activa de su columna).
    pub fn is_visible(&self, view: CenterView) -> bool {
        self.columns.iter().any(|c| c.active == view)
    }

    /// La hace visible dentro de su columna.
    pub fn activate(&mut self, view: CenterView) {
        let i = self.column_of(view);
        if let Some(c) = self.columns.get_mut(i) {
            c.active = view;
        }
    }

    /// Mueve la vista al destino y la deja visible. Si su columna queda vacía,
    /// desaparece; soltarla sobre sí misma no cambia nada.
    pub fn move_view(&mut self, view: CenterView, drop: DropTarget) {
        let from = self.column_of(view);
        let alone = self.columns[from].tabs.len() == 1;
        let target = match drop {
            DropTarget::Into(c) | DropTarget::Split { column: c, .. } => c,
        };
        if target >= self.columns.len() {
            return;
        }
        match drop {
            DropTarget::Into(c) if c == from => {
                self.activate(view);
                return;
            }
            DropTarget::Split { column, .. } if alone && column == from => return,
            _ => {}
        }

        // Sacarla de su columna (y quitar la columna si se queda vacía).
        let col = &mut self.columns[from];
        col.tabs.retain(|&v| v != view);
        if col.active == view {
            if let Some(&first) = col.tabs.first() {
                col.active = first;
            }
        }
        let removed = col.tabs.is_empty();
        if removed {
            self.columns.remove(from);
        }
        let shift = |c: usize| if removed && c > from { c - 1 } else { c };

        match drop {
            DropTarget::Into(c) => {
                let col = &mut self.columns[shift(c)];
                col.tabs.push(view);
                col.active = view;
            }
            DropTarget::Split { column, after } => {
                let at = shift(column) + usize::from(after);
                self.columns.insert(
                    at,
                    Column {
                        tabs: vec![view],
                        active: view,
                    },
                );
            }
        }
    }

    /// Columnas separadas por `|`, pestañas por `,`; la activa lleva `*`.
    pub fn serialize(&self) -> String {
        self.columns
            .iter()
            .map(|c| {
                c.tabs
                    .iter()
                    .map(|&v| {
                        let mark = if v == c.active { "*" } else { "" };
                        format!("{mark}{}", v.key())
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .collect::<Vec<_>>()
            .join("|")
    }

    /// Lo guardado, o el reparto por defecto si no cumple los invariantes.
    pub fn parse(text: &str) -> Self {
        let parse = || -> Option<Self> {
            let mut seen = Vec::new();
            let mut columns = Vec::new();
            for col in text.trim().split('|') {
                let mut tabs = Vec::new();
                let mut active = None;
                for tab in col.split(',') {
                    let (is_active, key) = match tab.trim().strip_prefix('*') {
                        Some(k) => (true, k),
                        None => (false, tab.trim()),
                    };
                    let view = CenterView::from_key(key)?;
                    if seen.contains(&view) {
                        return None;
                    }
                    seen.push(view);
                    tabs.push(view);
                    if is_active {
                        active = Some(view);
                    }
                }
                let active = active.or(tabs.first().copied())?;
                columns.push(Column { tabs, active });
            }
            (seen.len() == CenterView::ALL.len()).then_some(Self { columns })
        };
        parse().unwrap_or_default()
    }
}

/// Paneles laterales que se pueden acoplar a cualquier lado y redimensionar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Features,
    Explorer,
}

impl Pane {
    pub fn label(self) -> &'static str {
        match self {
            Pane::Features => "Features",
            Pane::Explorer => "Explorador",
        }
    }

    fn other(self) -> Pane {
        match self {
            Pane::Features => Pane::Explorer,
            Pane::Explorer => Pane::Features,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    fn flip(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dock {
    pub side: Side,
    /// Ancho en px, siempre dentro de [`PANE_MIN`, `PANE_MAX`].
    pub width: i32,
}

pub const PANE_MIN: i32 = 180;
pub const PANE_MAX: i32 = 640;
/// Ancho que se le reserva a la vista central al redimensionar (ver `.pane-center` en style.css).
pub const CENTER_MIN: i32 = 320;
pub const PANES_STORAGE_KEY: &str = "gryphus.panes";

/// Posición y ancho de los paneles laterales. Ambos pueden quedar del mismo lado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panes {
    pub features: Dock,
    pub explorer: Dock,
}

impl Default for Panes {
    fn default() -> Self {
        Self {
            features: Dock {
                side: Side::Left,
                width: 280,
            },
            explorer: Dock {
                side: Side::Right,
                width: 260,
            },
        }
    }
}

impl Panes {
    pub fn get(&self, pane: Pane) -> Dock {
        match pane {
            Pane::Features => self.features,
            Pane::Explorer => self.explorer,
        }
    }

    fn get_mut(&mut self, pane: Pane) -> &mut Dock {
        match pane {
            Pane::Features => &mut self.features,
            Pane::Explorer => &mut self.explorer,
        }
    }

    pub fn default_width(pane: Pane) -> i32 {
        Self::default().get(pane).width
    }

    /// Ancho máximo de `pane` con la ventana de ancho `viewport`: deja sitio al
    /// otro panel y a la vista central, sin salir nunca de [`PANE_MIN`, `PANE_MAX`].
    pub fn max_width(&self, pane: Pane, viewport: i32) -> i32 {
        let other = self.get(pane.other()).width;
        viewport
            .saturating_sub(other)
            .saturating_sub(CENTER_MIN)
            .clamp(PANE_MIN, PANE_MAX)
    }

    pub fn set_width(&mut self, pane: Pane, width: i32, viewport: i32) {
        let max = self.max_width(pane, viewport);
        self.get_mut(pane).width = width.clamp(PANE_MIN, max);
    }

    pub fn toggle_side(&mut self, pane: Pane) {
        let dock = self.get_mut(pane);
        dock.side = dock.side.flip();
    }

    /// Orden CSS (`order` de flexbox) del panel y de su divisor. La vista central
    /// tiene [`CENTER_ORDER`]; el divisor queda siempre del lado de la vista central.
    /// Con ambos paneles del mismo lado, Features va antes que el Explorador.
    pub fn order(&self, pane: Pane) -> (i32, i32) {
        let slot = match pane {
            Pane::Features => 0,
            Pane::Explorer => 2,
        };
        match self.get(pane).side {
            Side::Left => (slot, slot + 1),
            Side::Right => (CENTER_ORDER + 2 + slot, CENTER_ORDER + 1 + slot),
        }
    }

    /// Formato `clave=valor` por línea; tolera claves desconocidas y valores inválidos.
    pub fn serialize(&self) -> String {
        format!(
            "features_side={}\nfeatures_width={}\nexplorer_side={}\nexplorer_width={}\n",
            self.features.side.key(),
            self.features.width,
            self.explorer.side.key(),
            self.explorer.width
        )
    }

    pub fn parse(text: &str) -> Self {
        let mut p = Self::default();
        let side = |v: &str| match v {
            "left" => Some(Side::Left),
            "right" => Some(Side::Right),
            _ => None,
        };
        for (key, value) in text.lines().filter_map(|l| l.split_once('=')) {
            let value = value.trim();
            let (pane, field) = match key.trim().split_once('_') {
                Some(("features", f)) => (Pane::Features, f),
                Some(("explorer", f)) => (Pane::Explorer, f),
                _ => continue,
            };
            let dock = p.get_mut(pane);
            match field {
                "side" => {
                    if let Some(s) = side(value) {
                        dock.side = s;
                    }
                }
                "width" => {
                    if let Ok(w) = value.parse::<i32>() {
                        dock.width = w.clamp(PANE_MIN, PANE_MAX);
                    }
                }
                _ => {}
            }
        }
        p
    }
}

pub const CENTER_ORDER: i32 = 10;

#[cfg(test)]
mod tests {
    use super::*;

    use CenterView::{Agent, Diff, Editor};

    fn tabs(l: &CenterLayout) -> Vec<Vec<CenterView>> {
        l.columns.iter().map(|c| c.tabs.clone()).collect()
    }

    #[test]
    fn split_and_merge_back_to_the_initial_layout() {
        let mut l = CenterLayout::default();
        // Editor a la derecha: Agente | Editor (y Diff sigue con Agente).
        l.move_view(
            Editor,
            DropTarget::Split {
                column: 0,
                after: true,
            },
        );
        assert_eq!(tabs(&l), vec![vec![Agent, Diff], vec![Editor]]);
        assert!(l.is_visible(Agent) && l.is_visible(Editor) && !l.is_visible(Diff));
        // Diff a la izquierda de todo: Diff | Agente | Editor.
        l.move_view(
            Diff,
            DropTarget::Split {
                column: 0,
                after: false,
            },
        );
        assert_eq!(tabs(&l), vec![vec![Diff], vec![Agent], vec![Editor]]);
        // De vuelta a la barra superior de Agente: las columnas vacías desaparecen.
        l.move_view(Diff, DropTarget::Into(1));
        l.move_view(Editor, DropTarget::Into(0));
        assert_eq!(tabs(&l), vec![vec![Agent, Diff, Editor]]);
        assert_eq!(l.columns[0].active, Editor);
    }

    #[test]
    fn dropping_on_itself_changes_nothing() {
        let mut l = CenterLayout::default();
        l.move_view(
            Editor,
            DropTarget::Split {
                column: 0,
                after: false,
            },
        );
        let before = l.clone();
        for after in [false, true] {
            l.move_view(Editor, DropTarget::Split { column: 0, after });
            assert_eq!(l, before);
        }
        l.move_view(Editor, DropTarget::Into(0));
        assert_eq!(l, before);
        l.move_view(Editor, DropTarget::Into(9));
        assert_eq!(l, before);
    }

    #[test]
    fn moving_the_active_tab_activates_a_neighbour() {
        let mut l = CenterLayout::default();
        l.move_view(
            Agent,
            DropTarget::Split {
                column: 0,
                after: true,
            },
        );
        assert_eq!(l.columns[0].active, Editor);
        l.activate(Diff);
        assert_eq!(l.columns[0].active, Diff);
        assert_eq!(l.column_of(Agent), 1);
    }

    #[test]
    fn center_layout_round_trips_and_rejects_invalid() {
        let mut l = CenterLayout::default();
        l.move_view(
            Diff,
            DropTarget::Split {
                column: 0,
                after: true,
            },
        );
        l.activate(Editor);
        assert_eq!(CenterLayout::parse(&l.serialize()), l);
        for bad in [
            "",
            "agent,editor",
            "agent|agent,editor,diff",
            "agent,x|diff,editor",
        ] {
            assert_eq!(CenterLayout::parse(bad), CenterLayout::default(), "{bad}");
        }
    }

    #[test]
    fn panes_round_trip_and_tolerate_garbage() {
        let mut p = Panes::default();
        p.toggle_side(Pane::Explorer);
        p.set_width(Pane::Features, 333, 1600);
        assert_eq!(Panes::parse(&p.serialize()), p);
        let q = Panes::parse("features_side=arriba\nexplorer_width=99999\nbasura\nx_y=1");
        assert_eq!(q.features.side, Side::Left);
        assert_eq!(q.explorer.width, PANE_MAX);
        assert_eq!(Panes::parse(""), Panes::default());
    }

    #[test]
    fn widths_are_clamped_and_leave_room_for_the_center() {
        let mut p = Panes::default();
        p.set_width(Pane::Features, 10, 1600);
        assert_eq!(p.features.width, PANE_MIN);
        p.set_width(Pane::Features, 5000, 4000);
        assert_eq!(p.features.width, PANE_MAX);
        // Ventana estrecha: 1000 - 260 (explorador) - 320 (centro) = 420.
        p.set_width(Pane::Features, 600, 1000);
        assert_eq!(p.features.width, 420);
        // Ventana diminuta: nunca por debajo del mínimo.
        p.set_width(Pane::Features, 600, 300);
        assert_eq!(p.features.width, PANE_MIN);
    }

    #[test]
    fn order_keeps_handles_next_to_the_center() {
        let mut p = Panes::default();
        assert_eq!(p.order(Pane::Features), (0, 1));
        let (panel, handle) = p.order(Pane::Explorer);
        assert!(CENTER_ORDER < handle && handle < panel);
        // Ambos a la derecha: centro | divisor F | F | divisor E | E.
        p.toggle_side(Pane::Features);
        let (fp, fh) = p.order(Pane::Features);
        let (ep, eh) = p.order(Pane::Explorer);
        assert!(CENTER_ORDER < fh && fh < fp && fp < eh && eh < ep);
        // Ambos a la izquierda: F | divisor F | E | divisor E | centro.
        p.toggle_side(Pane::Features);
        p.toggle_side(Pane::Explorer);
        let (fp, fh) = p.order(Pane::Features);
        let (ep, eh) = p.order(Pane::Explorer);
        assert!(fp < fh && fh < ep && ep < eh && eh < CENTER_ORDER);
    }

    #[test]
    fn center_view_has_three_tabs_in_order() {
        let labels: Vec<_> = CenterView::ALL.iter().map(|v| v.label()).collect();
        assert_eq!(labels, ["Agente", "Editor", "Diff"]);
    }

    #[test]
    fn shortcuts_are_unique() {
        for mac in [false, true] {
            let mut shortcuts: Vec<_> = CenterView::ALL.iter().map(|v| v.shortcut(mac)).collect();
            shortcuts.dedup();
            assert_eq!(shortcuts.len(), CenterView::ALL.len());
        }
    }

    #[test]
    fn mac_shortcuts_use_command() {
        assert!(CenterView::ALL
            .iter()
            .all(|v| v.shortcut(true).starts_with('⌘')));
        assert!(CenterView::ALL
            .iter()
            .all(|v| v.shortcut(false).starts_with("Ctrl+")));
    }
}
