//! Arnés e2e: compila la app con el frontend embebido, arranca `tauri-driver`
//! (que lanza la app real dentro de su WebView) y ofrece ayudas de espera.

#![allow(dead_code)]

use std::future::Future;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use fantoccini::{Client, ClientBuilder, Locator};
use gryphus_testkit::{fake_agent_bin, TempRepo};
use serde_json::{json, Value};

pub const TIMEOUT: Duration = Duration::from_secs(30);

/// Secuencias de teclas WebDriver (W3C).
pub const ENTER: &str = "\u{E007}";
pub const CTRL: &str = "\u{E009}";
pub const END: &str = "\u{E010}";
pub const TAB: &str = "\u{E004}";
pub const SHIFT: &str = "\u{E008}";
pub const RELEASE: &str = "\u{E000}";
pub const ESCAPE: &str = "\u{E00C}";

pub fn enabled() -> bool {
    std::env::var("GRYPHUS_E2E").is_ok_and(|v| v == "1")
}

/// Las pruebas e2e comparten puertos y ventanas: se ejecutan de una en una.
pub static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("e2e está en la raíz del workspace")
        .to_path_buf()
}

fn run(cmd: &mut Command) {
    let status = cmd
        .status()
        .unwrap_or_else(|e| panic!("no se pudo ejecutar {cmd:?}: {e}"));
    assert!(status.success(), "falló {cmd:?}");
}

/// Compila la UI y la app con el frontend embebido (una vez por proceso).
/// `release` se usa para medir rendimiento.
pub fn app_binary(release: bool) -> PathBuf {
    static DEBUG: OnceLock<PathBuf> = OnceLock::new();
    static RELEASE: OnceLock<PathBuf> = OnceLock::new();
    let cell = if release { &RELEASE } else { &DEBUG };
    cell.get_or_init(|| {
        let ws = workspace();
        let target_dir = if release {
            "target/e2e-release"
        } else {
            "target/e2e"
        };
        let mut trunk = vec!["--config", "ui/Trunk.toml", "build"];
        let mut cargo_args = vec![
            "build",
            "-p",
            "gryphus-desktop",
            "--features",
            "custom-protocol",
            "--target-dir",
            target_dir,
        ];
        if release {
            trunk.push("--release");
            cargo_args.push("--release");
        }
        run(Command::new("trunk").args(&trunk).current_dir(&ws));
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        run(Command::new(cargo).args(&cargo_args).current_dir(&ws));
        let exe = if cfg!(windows) {
            "gryphus.exe"
        } else {
            "gryphus"
        };
        let profile = if release { "release" } else { "debug" };
        ws.join(target_dir).join(profile).join(exe)
    })
    .clone()
}

/// `msedgedriver` (Windows) igual a la versión de WebView2. En Linux no hace falta.
fn native_driver() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("GRYPHUS_NATIVE_DRIVER") {
        return Some(path.into());
    }
    if !cfg!(windows) {
        return None;
    }
    let tools = workspace().join("target").join("e2e-tools");
    std::fs::read_dir(tools)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path().join("msedgedriver.exe"))
        .find(|p| p.is_file())
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("puerto libre")
}

/// Repo temporal + directorio de datos con los tres proveedores apuntando a fake-agent.
pub struct Fixture {
    pub repo: TempRepo,
    pub data: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let repo = TempRepo::new();
        let data = repo.base().join("appdata");
        std::fs::create_dir_all(&data).unwrap();
        let bin = fake_agent_bin();
        let config: String = ["opencode", "claude", "codex"]
            .iter()
            .map(|p| format!("[agents.{p}]\ncommand = '{}'\n\n", bin.display()))
            .collect();
        std::fs::write(data.join("config.toml"), config).unwrap();
        Self { repo, data }
    }

    pub fn root(&self) -> String {
        self.repo.root().to_string_lossy().into_owned()
    }
}

pub struct App {
    pub client: Client,
    driver: tokio::process::Child,
}

pub async fn wait_for<F, Fut>(what: &str, mut cond: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let start = Instant::now();
    while start.elapsed() < TIMEOUT {
        if cond().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("tiempo agotado esperando: {what}");
}

impl App {
    pub async fn start(fixture: &Fixture) -> Self {
        Self::start_with(fixture, false).await
    }

    pub async fn start_with(fixture: &Fixture, release: bool) -> Self {
        let exe = app_binary(release);
        let port = free_port();
        let native_port = free_port();
        let mut cmd = tokio::process::Command::new("tauri-driver");
        cmd.args([
            "--port",
            &port.to_string(),
            "--native-port",
            &native_port.to_string(),
        ])
        .env("GRYPHUS_DATA_DIR", &fixture.data)
        .kill_on_drop(true);
        // Log de tauri-driver/msedgedriver para diagnosticar fallos de sesión.
        let logs = workspace().join("target").join("e2e-logs");
        std::fs::create_dir_all(&logs).unwrap();
        let log_path = logs.join(format!("tauri-driver-{port}.log"));
        let log = std::fs::File::create(&log_path).unwrap();
        cmd.stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log));
        if let Some(native) = native_driver() {
            cmd.arg("--native-driver").arg(native);
        }
        let driver = cmd
            .spawn()
            .expect("no se pudo lanzar tauri-driver (cargo install tauri-driver --locked)");
        let addr = format!("127.0.0.1:{port}");
        wait_for("tauri-driver", || async {
            TcpStream::connect(&addr).is_ok()
        })
        .await;

        let mut caps = serde_json::Map::new();
        caps.insert("browserName".into(), json!("wry"));
        caps.insert("tauri:options".into(), json!({ "application": exe }));
        // Sin límite, un fallo al lanzar la app deja la prueba colgada indefinidamente.
        let client = tokio::time::timeout(
            Duration::from_secs(60),
            ClientBuilder::native()
                .capabilities(caps)
                .connect(&format!("http://{addr}")),
        )
        .await
        .unwrap_or_else(|_| {
            panic!(
                "la sesión WebDriver no se abrió en 60 s; log: {}",
                log_path.display()
            )
        })
        .expect("sesión WebDriver con la app");
        Self { client, driver }
    }

    /// Cierra la ventana (la app guarda y detiene sus agentes) y el driver.
    pub async fn quit(mut self) {
        let _ = self.client.close().await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let _ = self.driver.kill().await;
    }

    pub async fn exists(&self, css: &str) -> bool {
        self.client.find(Locator::Css(css)).await.is_ok()
    }

    pub async fn wait_css(&self, css: &str) {
        wait_for(css, || self.exists(css)).await;
    }

    pub async fn wait_gone(&self, css: &str) {
        wait_for(&format!("que desaparezca {css}"), || async {
            !self.exists(css).await
        })
        .await;
    }

    pub async fn text(&self, css: &str) -> String {
        match self.client.find(Locator::Css(css)).await {
            Ok(el) => el.text().await.unwrap_or_default(),
            Err(_) => String::new(),
        }
    }

    pub async fn wait_text(&self, css: &str, needle: &str) {
        wait_for(&format!("{needle:?} en {css}"), || async {
            self.text(css).await.contains(needle)
        })
        .await;
    }

    pub async fn click(&self, css: &str) {
        self.wait_css(css).await;
        self.client
            .find(Locator::Css(css))
            .await
            .unwrap()
            .click()
            .await
            .unwrap_or_else(|e| panic!("clic en {css}: {e}"));
    }

    /// Clic derecho (evento `contextmenu`) sobre el primer elemento que cumple `css`.
    pub async fn right_click(&self, css: &str) {
        self.wait_css(css).await;
        self.script(
            "const el = document.querySelector(arguments[0]);
             const r = el.getBoundingClientRect();
             el.dispatchEvent(new MouseEvent('contextmenu', {
                 bubbles: true, cancelable: true, button: 2,
                 clientX: r.left + 10, clientY: r.top + r.height / 2 }));",
            vec![json!(css)],
        )
        .await;
        self.wait_css(".context-menu").await;
    }

    /// Elige `value` en un `<select>` como lo haría el usuario (dispara `change`).
    pub async fn select_value(&self, css: &str, value: &str) {
        self.wait_css(css).await;
        self.script(
            "const el = document.querySelector(arguments[0]);
             el.value = arguments[1];
             el.dispatchEvent(new Event('change', { bubbles: true }));",
            vec![json!(css), json!(value)],
        )
        .await;
    }

    /// Estilo calculado de `prop` en el primer elemento que cumple `css`.
    pub async fn computed_style(&self, css: &str, prop: &str) -> String {
        self.wait_css(css).await;
        self.script(
            "return getComputedStyle(document.querySelector(arguments[0]))
                 .getPropertyValue(arguments[1]);",
            vec![json!(css), json!(prop)],
        )
        .await
        .as_str()
        .unwrap_or_default()
        .to_owned()
    }

    /// Espera a que el campo de renombrar dentro de `scope` tenga el foco.
    pub async fn wait_rename_focus(&self, scope: &str) {
        let input = format!("{scope} .rename-input");
        self.wait_css(&input).await;
        wait_for("foco en el campo de renombrar", || async {
            self.script(
                "return document.activeElement === document.querySelector(arguments[0]);",
                vec![json!(input)],
            )
            .await
                == json!(true)
        })
        .await;
    }

    /// Escribe en el campo de renombrar con foco (reemplaza el texto) y confirma.
    pub async fn rename_to(&self, scope: &str, name: &str) {
        self.wait_rename_focus(scope).await;
        let input = format!("{scope} .rename-input");
        self.type_active(&format!("{CTRL}a{RELEASE}{name}{ENTER}"))
            .await;
        self.wait_gone(&input).await;
    }

    pub async fn click_xpath(&self, xpath: &str) {
        wait_for(xpath, || async {
            self.client.find(Locator::XPath(xpath)).await.is_ok()
        })
        .await;
        self.client
            .find(Locator::XPath(xpath))
            .await
            .unwrap()
            .click()
            .await
            .unwrap();
    }

    /// Escribe en el elemento con foco (tras hacer clic en `css`).
    pub async fn type_into(&self, css: &str, keys: &str) {
        self.click(css).await;
        self.type_active(keys).await;
    }

    pub async fn type_active(&self, keys: &str) {
        self.client
            .active_element()
            .await
            .unwrap()
            .send_keys(keys)
            .await
            .unwrap();
    }

    /// Como `wait_text`, pero con el `textContent` del DOM. El texto "renderizado"
    /// de WebDriver varía entre motores (WebKitGTK devuelve vacío en algunos
    /// elementos con `overflow: hidden`).
    pub async fn wait_dom_text(&self, css: &str, needle: &str) {
        wait_for(&format!("{needle:?} en {css} (DOM)"), || async {
            self.script(
                "const e = document.querySelector(arguments[0]); return e ? e.textContent : '';",
                vec![json!(css)],
            )
            .await
            .as_str()
            .is_some_and(|t| t.contains(needle))
        })
        .await;
    }

    pub async fn script(&self, js: &str, args: Vec<Value>) -> Value {
        self.client.execute(js, args).await.expect("script")
    }

    pub async fn terminal_text(&self) -> String {
        self.script("return window.GryphusJs.debugTerminalText();", vec![])
            .await
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }

    pub async fn wait_terminal(&self, needle: &str) {
        wait_for(&format!("{needle:?} en la terminal"), || async {
            self.terminal_text().await.contains(needle)
        })
        .await;
    }

    /// Las confirmaciones (modal de la UI) se aceptan automáticamente y se registran.
    pub async fn auto_confirm(&self) {
        self.script(
            "window.__confirms = []; window.__gryphusAutoConfirm = true; return null;",
            vec![],
        )
        .await;
    }

    pub async fn invoke(&self, cmd: &str, req: Value) -> Value {
        self.script(
            "return window.__TAURI__.core.invoke(arguments[0], { req: arguments[1] });",
            vec![json!(cmd), req],
        )
        .await
    }

    /// Como [`Self::invoke`], pero un comando que falla devuelve su `IpcError`
    /// (`{ code, message }`) en vez de hacer fallar el script.
    pub async fn invoke_result(&self, cmd: &str, req: Value) -> Result<Value, Value> {
        let res = self
            .script(
                "return window.__TAURI__.core.invoke(arguments[0], { req: arguments[1] })
                     .then(ok => ({ ok }), err => ({ err }));",
                vec![json!(cmd), req],
            )
            .await;
        match res.get("err") {
            Some(err) => Err(err.clone()),
            None => Ok(res["ok"].clone()),
        }
    }

    /// Registra lo que la app escribe en el portapapeles (y lo sigue escribiendo
    /// en el del sistema). Leerlo con [`Self::wait_copied`].
    pub async fn record_clipboard(&self) {
        self.script(
            "window.__copied = [];
             const clip = navigator.clipboard;
             const original = clip.writeText.bind(clip);
             clip.writeText = text => { window.__copied.push(text); return original(text); };
             return null;",
            vec![],
        )
        .await;
    }

    /// Espera la siguiente escritura en el portapapeles y la devuelve.
    pub async fn wait_copied(&self) -> String {
        let start = Instant::now();
        while start.elapsed() < TIMEOUT {
            let next = self
                .script("return window.__copied.shift() ?? null;", vec![])
                .await;
            if let Some(text) = next.as_str() {
                return text.to_owned();
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("tiempo agotado esperando: escritura en el portapapeles");
    }

    pub async fn open_project(&self, path: &str) {
        self.wait_css("#project-path").await;
        self.client
            .find(Locator::Css("#project-path"))
            .await
            .unwrap()
            .send_keys(path)
            .await
            .unwrap();
        self.click("#open-path").await;
        self.wait_css(".shell").await;
    }

    pub async fn screenshot(&self, name: &str) {
        let dir = workspace().join("target").join("e2e-screens");
        std::fs::create_dir_all(&dir).unwrap();
        if let Ok(png) = self.client.screenshot().await {
            std::fs::write(dir.join(format!("{name}.png")), png).unwrap();
        }
    }
}
