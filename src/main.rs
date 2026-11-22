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

/// The file name after a command, refused when missing or when it is
/// really the next option.
fn file_argument<'a>(args: &'a [String], usage: &str) -> Result<&'a String> {
    match args.get(1) {
        Some(file) if !file.starts_with("--") => Ok(file),
        _ => bail!("Informe o nome do arquivo. Uso: {usage}"),
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
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
                .context("Informe o caminho do arquivo de importação")?;
            let mut bytes = Vec::new();
            std::fs::File::open(file)?
                .take(native::MAX_FRAME as u64 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > native::MAX_FRAME {
                bail!(
                    "O arquivo de importação é maior que {} MB",
                    native::MAX_FRAME / (1024 * 1024)
                );
            }
            let capture: Capture = serde_json::from_slice(&bytes)
                .map_err(|_| anyhow::anyhow!("O arquivo de importação é inválido"))?;
            let receipt = Store::open(&paths::db_path()?)?.import_capture(&capture)?;
            println!("{}", serde_json::to_string(&receipt)?);
            Ok(())
        }
        Some("--import-plan") => {
            let file = file_argument(&args, "--import-plan ARQUIVO.json [--dry-run]")?;
            if args.len() > 3 || (args.len() == 3 && args[2] != "--dry-run") {
                bail!("Uso: --import-plan ARQUIVO.json [--dry-run]");
            }
            let mut bytes = Vec::new();
            std::fs::File::open(file)?
                .take(native::MAX_FRAME as u64 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > native::MAX_FRAME {
                bail!("O arquivo de importação é grande demais.");
            }
            let plan: ponteproj::plan_import::Plan =
                serde_json::from_slice(&bytes).context("JSON do plano inválido")?;
            let result = Store::open(&paths::db_path()?)?.import_plan(&plan, args.len() == 3)?;
            println!("{}", serde_json::to_string(&result)?);
            Ok(())
        }
        Some("--import-templates") => {
            let file = file_argument(&args, "--import-templates ARQUIVO.json [--publicar]")?;
            let publish = args.iter().skip(2).any(|a| a == "--publicar");
            let bundle = ponteproj::template_bundle::read_bundle(std::path::Path::new(file))?;
            let report =
                Store::open(&paths::db_path()?)?.import_template_bundle(&bundle, publish)?;
            if !bundle.blocks.is_empty() || !bundle.templates.is_empty() {
                println!(
                    "{} e {} criados{}.",
                    plural(report.blocks_created, "instrução", "instruções"),
                    plural(report.templates_created, "modelo", "modelos"),
                    if publish {
                        " e publicados"
                    } else {
                        " como rascunho"
                    }
                );
            }
            if report.examples_created > 0 {
                println!(
                    "{}: aprove-{o} na área Modelos para que a IA {o} use.",
                    if report.examples_created == 1 {
                        "1 exemplo criado como não conferido".to_owned()
                    } else {
                        format!(
                            "{} exemplos criados como não conferidos",
                            report.examples_created
                        )
                    },
                    o = if report.examples_created == 1 {
                        "o"
                    } else {
                        "os"
                    }
                );
            }
            for published in &report.published {
                println!("Publicado (já estava no acervo): {published}");
            }
            for kept in &report.kept {
                println!("Mantido como estava: {kept}");
            }
            Ok(())
        }
        Some("--export-templates") => {
            let file = file_argument(&args, "--export-templates ARQUIVO.json [--com-exemplos]")?;
            let with_examples = args.iter().skip(2).any(|a| a == "--com-exemplos");
            let store = Store::open(&paths::db_path()?)?;
            let (selection, skipped) = store.selection_of_everything(with_examples)?;
            for line in &skipped {
                println!("{line}");
            }
            let bundle = store.export_template_bundle(&selection)?;
            ponteproj::template_bundle::write_bundle(std::path::Path::new(file), &bundle)?;
            let blocks = plural(bundle.blocks.len(), "instrução", "instruções");
            let templates = plural(bundle.templates.len(), "modelo", "modelos");
            if with_examples {
                let examples = plural(bundle.examples.len(), "exemplo", "exemplos");
                println!("{blocks}, {templates} e {examples} gravados em {file}.");
            } else {
                println!("{blocks} e {templates} gravados em {file}.");
            }
            Ok(())
        }
        // The installers' safety copy: no window, no migration, and the
        // library is only read (see `upgrade_backup`).
        Some(ponteproj::upgrade_backup::COMMAND) => ponteproj::upgrade_backup::run(&args[1..]),
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
        Some("--templates") => app::run_opening(None, true),
        #[cfg(feature = "shots")]
        Some("--shots") => {
            let out = args.get(1).context("Informe a pasta das imagens")?;
            ponteproj::shots::prepare(out.into(), args.get(2).cloned().unwrap_or_default())?;
            app::run(None)
        }
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
                "PonteProj: computador principal ativo na rede interna em http://{}\nChave de acesso do gabinete: {}",
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
                std::fs::write(
                    file,
                    guide::to_html(guide::GUIDE, env!("CARGO_PKG_VERSION")),
                )
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
            println!("PonteProj\n  sem argumentos: abrir aplicativo\n  --open-document ID\n  --templates (abre na área Modelos)\n  --import-json ARQUIVO\n  --import-plan ARQUIVO.json [--dry-run]\n  --import-templates ARQUIVO.json [--publicar]\n  --export-templates ARQUIVO.json [--com-exemplos]\n  --list [BUSCA]\n  --serve [ENDEREÇO:PORTA]\n  --guide [ARQUIVO.html]\n  --native-host\n\nAcervo: perfil local do Windows; PONTEPROJ_DATA_DIR pode definir outra pasta.");
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
