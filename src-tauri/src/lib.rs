//! Composition root de Gryphus: el único lugar que conoce los adaptadores concretos
//! y los inyecta en los casos de uso. Ver docs/architecture/ipc.md.
//!
//! Hito 0: solo arranca la ventana. Los comandos llegan en el hito 7.

pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error al iniciar Gryphus");
}
