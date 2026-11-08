#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use anyhow::{bail, Context, Result};
use ponteproj::{app, guide, model::Capture, native, paths, storage::Store};
use std::io::Read;

fn main() {
    if let Err(err) = run() {
        eprintln!("PonteProj: {err:#}");
        #[cfg(windows)]
        if std::env::args().len() == 1
            || std::env::args().nth(1).as_deref() == Some("--open-document")
        {
            show_startup_error(&format!("Não foi possível abrir o PonteProj.\n\n{err}"));
        }
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(origin) = args
        .first()
        .filter(|s| s.starts_with("chrome-extension://"))
    {
        if !native::valid_extension_origin(origin) {
            // Reply in words rather than closing: the page shows them.
            return native::refuse_on_stdio(
                "origin_denied",
                "Esta extensão não é reconhecida por este Ponte. Instale a versão mais recente do Ponte (github.com/demusis/ponteproj/releases) com a sua conta do Windows e reinicie o navegador.",
            );
        }
        return native::run_host();
    }
    match args.first().map(String::as_str) {
        Some("--native-host") => native::run_host(),
        Some("--import-json") => {
            let file = args
                .get(1)
                .context("Informe o caminho do JSON de captura")?;
            let mut bytes = Vec::new();
            std::fs::File::open(file)?
                .take(native::MAX_FRAME as u64 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > native::MAX_FRAME {
                bail!(
                    "Arquivo de captura excede {} MiB",
                    native::MAX_FRAME / (1024 * 1024)
                );
            }
            let capture: Capture = serde_json::from_slice(&bytes)
                .map_err(|_| anyhow::anyhow!("JSON de captura inválido"))?;
            let receipt = Store::open(&paths::db_path()?)?.import_capture(&capture)?;
            println!("{}", serde_json::to_string(&receipt)?);
            Ok(())
        }
        Some("--list") => {
            let store = Store::open(&paths::db_path()?)?;
            println!(
                "{}",
                serde_json::to_string(
                    &store.list_documents(args.get(1).map(String::as_str).unwrap_or(""))?
                )?
            );
            Ok(())
        }
        Some("--open-document") => app::run(Some(
            args.get(1)
                .context("Informe o identificador local")?
                .parse()?,
        )),
        Some("--serve") => {
            let db_path = paths::db_path()?;
            let mut store = Store::open(&db_path)?;
            let mut config = store.load_intranet_config()?;
            config.mode = ponteproj::intranet::IntranetMode::Host;
            if let Some(addr) = args.get(1) {
                config.bind_addr = addr.clone();
            }
            if config.cabinet_token.trim().is_empty() {
                config.cabinet_token = ponteproj::intranet::generate_token();
            }
            store.save_intranet_config(&config)?;
            let bind_addr = config.bind_addr.clone();
            let server = ponteproj::intranet::HostServer::start(db_path, &bind_addr)?;
            println!(
                "PonteProj Anfitrião na intranet ativo em http://{}\nToken do gabinete: {}",
                server.local_addr(),
                config.cabinet_token
            );
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3600));
            }
        }
        // The user guide as a web page: written to the given file, or opened
        // in the browser when no file is named.
        Some("--guide") => match args.get(1) {
            Some(file) => {
                std::fs::write(file, guide::to_html(guide::GUIDE, env!("CARGO_PKG_VERSION")))
                    .context("Não foi possível gravar o guia no arquivo informado")?;
                Ok(())
            }
            None => guide::open().map(|_| ()),
        },
        Some("--version") => {
            println!("PonteProj {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("--help") => {
            println!("PonteProj\n  sem argumentos: abrir aplicativo\n  --open-document ID\n  --import-json ARQUIVO\n  --list [BUSCA]\n  --serve [ENDEREÇO:PORTA]\n  --guide [ARQUIVO.html]\n  --native-host\n\nAcervo: perfil local do Windows; PONTEPROJ_DATA_DIR pode definir outra pasta.");
            Ok(())
        }
        Some(_) => bail!("Argumento desconhecido. Use --help."),
        None => app::run(None),
    }
}

#[cfg(windows)]
fn show_startup_error(message: &str) {
    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(
            window: *mut std::ffi::c_void,
            text: *const u16,
            title: *const u16,
            flags: u32,
        ) -> i32;
    }
    let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    let title: Vec<u16> = "PonteProj".encode_utf16().chain(Some(0)).collect();
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), 0x10);
    }
}
