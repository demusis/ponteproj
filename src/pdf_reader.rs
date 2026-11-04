//! Native Windows PDF rendering and optional OCR, entirely in memory.
use crate::{protection, storage::Store};
use anyhow::{ensure, Context, Result};
use eframe::egui;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::sync::mpsc::{self, Receiver};
use windows::{
    Data::Pdf::{PdfDocument, PdfPageRenderOptions},
    Storage::Streams::{DataReader, DataWriter, InMemoryRandomAccessStream},
    Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PageText {
    pub text: String,
    pub language: String,
    pub created_at: String,
}
impl Store {
    pub(crate) fn ensure_page_text(&mut self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS page_text(file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,page INTEGER NOT NULL,payload BLOB NOT NULL,PRIMARY KEY(file_id,page));")?;
        Ok(())
    }
    pub fn pdf_bytes(&self, file: i64, capture: i64) -> Result<Vec<u8>> {
        let blob: Vec<u8> = self.conn.query_row(
            "SELECT payload FROM files WHERE id=?1 AND capture_id=?2",
            params![file, capture],
            |r| r.get(0),
        )?;
        protection::unseal(&blob)
    }
    pub fn save_page_text(&mut self, file: i64, page: u32, value: &PageText) -> Result<()> {
        ensure!(
            value.text.len() <= 64000 && value.language.len() <= 128 && page < 10000,
            "Texto OCR excede os limites."
        );
        if let Some(cfg) = self.client_config()? {
            self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::SavePageText {
                    file_id: file,
                    page,
                    value: value.clone(),
                },
            )?;
            return Ok(());
        }
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO page_text(file_id,page,payload) VALUES(?1,?2,?3) ON CONFLICT(file_id,page) DO UPDATE SET payload=excluded.payload",params![file,page,protection::seal(&serde_json::to_vec(value)?)?])?;
        crate::intranet::record_mutation(
            &tx,
            &actor,
            "page_text",
            file,
            None,
            "save_page_text",
            None,
            &format!("Salvou OCR do arquivo #{file} (pág. {})", page + 1),
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn page_texts_for_file(&self, file: i64) -> Result<Vec<(u32, PageText)>> {
        let (metadata,document):(Vec<u8>,i64)=self.conn.query_row("SELECT f.metadata,c.document_id FROM files f JOIN captures c ON c.id=f.capture_id WHERE f.id=?1",[file],|r|Ok((r.get(0)?,r.get(1)?)))?;
        let current: crate::files::FileSummary =
            serde_json::from_slice(&protection::unseal(&metadata)?)?;
        let mut statement=self.conn.prepare("SELECT f.id,f.metadata,p.page,p.payload FROM files f JOIN captures c ON c.id=f.capture_id JOIN page_text p ON p.file_id=f.id WHERE c.document_id=?1 ORDER BY (f.id=?2) DESC,f.id DESC")?;
        let rows = statement.query_map(params![document, file], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, u32>(2)?,
                r.get::<_, Vec<u8>>(3)?,
            ))
        })?;
        let mut pages = std::collections::BTreeMap::new();
        for row in rows {
            let (_, metadata, page, payload) = row?;
            let summary: crate::files::FileSummary =
                serde_json::from_slice(&protection::unseal(&metadata)?)?;
            if summary.sha256 == current.sha256 && !pages.contains_key(&page) {
                pages.insert(
                    page,
                    serde_json::from_slice(&protection::unseal(&payload)?)?,
                );
            }
        }
        Ok(pages.into_iter().collect())
    }
    pub fn load_page_text(&self, file: i64, page: u32) -> Result<Option<PageText>> {
        let bytes: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT payload FROM page_text WHERE file_id=?1 AND page=?2",
                params![file, page],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(bytes) = bytes {
            return Ok(Some(serde_json::from_slice(&protection::unseal(&bytes)?)?));
        }
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM files WHERE id=?1)",
            [file],
            |r| r.get(0),
        )?;
        if !exists {
            return Ok(None);
        }
        Ok(self
            .page_texts_for_file(file)?
            .into_iter()
            .find(|(number, _)| *number == page)
            .map(|(_, text)| text))
    }
}
struct Runtime;
impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe { RoUninitialize() }
    }
}
pub struct Rendered {
    pub image: egui::ColorImage,
    pub pages: u32,
    pub ocr: Option<std::result::Result<PageText, String>>,
}
pub fn render(bytes: &[u8], index: u32, width: u32, ocr: bool) -> Result<Rendered> {
    ensure!(
        bytes.len() <= crate::files::MAX_FILE_BYTES
            && bytes.starts_with(b"%PDF-")
            && (400..=2400).contains(&width),
        "PDF ou resolução inválida."
    );
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
        .context("Não foi possível iniciar o leitor nativo")?;
    let _runtime = Runtime;
    let input = InMemoryRandomAccessStream::new()?;
    let writer = DataWriter::CreateDataWriter(&input.GetOutputStreamAt(0)?)?;
    writer.WriteBytes(bytes)?;
    writer.StoreAsync()?.join()?;
    writer.DetachStream()?;
    input.Seek(0)?;
    let pdf = PdfDocument::LoadFromStreamAsync(&input)?
        .join()
        .context("Não foi possível abrir o PDF (arquivo protegido por senha ou inválido)")?;
    let pages = pdf.PageCount()?;
    ensure!(
        pages > 0 && pages <= 10000 && index < pages,
        "Página fora do PDF ou quantidade excessiva de páginas."
    );
    let page = pdf.GetPage(index)?;
    let size = page.Size()?;
    ensure!(
        size.Width.is_finite() && size.Height.is_finite() && size.Width > 0.0 && size.Height > 0.0,
        "Dimensões de página inválidas."
    );
    let height = (width as f64 * size.Height as f64 / size.Width as f64).ceil();
    ensure!(
        (1.0..=6000.0).contains(&height) && width as f64 * height <= 12_000_000.0,
        "Página excede o limite de renderização; reduza a resolução."
    );
    let options = PdfPageRenderOptions::new()?;
    options.SetDestinationWidth(width)?;
    options.SetDestinationHeight(height as u32)?;
    let output = InMemoryRandomAccessStream::new()?;
    page.RenderWithOptionsToStreamAsync(&output, &options)?
        .join()?;
    page.Close()?;
    let length = output.Size()?;
    ensure!(
        length > 0 && length <= 32 * 1024 * 1024,
        "Imagem renderizada excede 32 MiB."
    );
    let reader = DataReader::CreateDataReader(&output.GetInputStreamAt(0)?)?;
    ensure!(
        reader.LoadAsync(length as u32)?.join()? == length as u32,
        "Imagem incompleta."
    );
    let mut png = vec![0; length as usize];
    reader.ReadBytes(&mut png)?;
    reader.DetachStream()?;
    let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)?.into_rgba8();
    ensure!(
        image.width() <= 2400 && image.height() <= 6000,
        "Dimensões renderizadas inesperadas."
    );
    let color = egui::ColorImage::from_rgba_unmultiplied(
        [image.width() as usize, image.height() as usize],
        image.as_raw(),
    );
    let result = if ocr {
        Some(
            (|| -> Result<PageText> {
                use windows::{
                    Graphics::Imaging::{BitmapDecoder, BitmapPixelFormat, SoftwareBitmap},
                    Media::Ocr::OcrEngine,
                };
                output.Seek(0)?;
                let decoder = BitmapDecoder::CreateAsync(&output)?.join()?;
                let bitmap = decoder.GetSoftwareBitmapAsync()?.join()?;
                let bitmap = SoftwareBitmap::Convert(&bitmap, BitmapPixelFormat::Gray8)?;
                let engine = OcrEngine::TryCreateFromUserProfileLanguages()
                    .context("Nenhum idioma OCR disponível para o perfil Windows")?;
                ensure!(
                    bitmap.PixelWidth()? as u32 <= OcrEngine::MaxImageDimension()?
                        && bitmap.PixelHeight()? as u32 <= OcrEngine::MaxImageDimension()?,
                    "Página excede o limite do OCR do Windows."
                );
                let recognized = engine.RecognizeAsync(&bitmap)?.join()?;
                let text = recognized.Text()?.to_string();
                ensure!(
                    text.len() <= 64000,
                    "Texto OCR excede 64 KB; não foi salvo parcialmente."
                );
                Ok(PageText {
                    text,
                    language: engine.RecognizerLanguage()?.LanguageTag()?.to_string(),
                    created_at: chrono::Utc::now().to_rfc3339(),
                })
            })()
            .map_err(|e| e.to_string()),
        )
    } else {
        None
    };
    Ok(Rendered {
        image: color,
        pages,
        ocr: result,
    })
}
struct Selection {
    file: i64,
    capture: i64,
    label: String,
    page: u32,
    width: u32,
    pages: u32,
    texture: Option<egui::TextureHandle>,
    text: Option<PageText>,
}
#[derive(Default)]
pub struct PdfPanel {
    selection: Option<Selection>,
    job: Option<Receiver<std::result::Result<Rendered, String>>>,
    open: bool,
    message: String,
}
impl PdfPanel {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
    pub fn open(
        &mut self,
        store: &Store,
        file: i64,
        capture: i64,
        label: &str,
        ctx: &egui::Context,
    ) {
        if self.job.is_some() {
            self.open = true;
            self.message = "Aguarde a leitura em andamento antes de abrir outro arquivo.".into();
            return;
        }
        self.selection = Some(Selection {
            file,
            capture,
            label: label.into(),
            page: 0,
            width: 1400,
            pages: 0,
            texture: None,
            text: None,
        });
        self.open = true;
        self.request(store, ctx, false);
    }
    fn request(&mut self, store: &Store, ctx: &egui::Context, ocr: bool) {
        if self.job.is_some() {
            return;
        }
        let Some(selection) = &mut self.selection else {
            return;
        };
        selection.texture = None;
        selection.text = None;
        let bytes = match store.pdf_bytes(selection.file, selection.capture) {
            Ok(b) => b,
            Err(e) => {
                self.message = e.to_string();
                return;
            }
        };
        let page = selection.page;
        let width = selection.width;
        selection.texture = None;
        selection.text = store.load_page_text(selection.file, page).ok().flatten();
        let (send, receive) = mpsc::channel();
        let repaint = ctx.clone();
        self.job = Some(receive);
        self.message = "Lendo página…".into();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(|| render(&bytes, page, width, ocr))
                .map_err(|_| "Falha no leitor nativo".to_string())
                .and_then(|r| r.map_err(|e| e.to_string()));
            let _ = send.send(result);
            repaint.request_repaint();
        });
    }
    pub fn show(&mut self, ctx: &egui::Context, store: &mut Store) {
        if let Some(job) = &self.job {
            match job.try_recv() {
                Ok(result) => {
                    self.job = None;
                    match result {
                        Ok(rendered) => {
                            if let Some(selection) = &mut self.selection {
                                selection.pages = rendered.pages;
                                selection.texture = Some(ctx.load_texture(
                                    "pdf-page",
                                    rendered.image,
                                    egui::TextureOptions::LINEAR,
                                ));
                                self.message.clear();
                                if let Some(result) = rendered.ocr {
                                    match result {
                                        Ok(text) => {
                                            match store.save_page_text(
                                                selection.file,
                                                selection.page,
                                                &text,
                                            ) {
                                                Ok(()) => {
                                                    self.message="OCR local salvo; confira o texto com a página original.".into();
                                                    selection.text = Some(text);
                                                }
                                                Err(e) => {
                                                    self.message = format!("OCR não salvo: {e}")
                                                }
                                            }
                                        }
                                        Err(e) => self.message = format!("OCR indisponível: {e}"),
                                    }
                                }
                            }
                        }
                        Err(e) => self.message = e,
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.job = None;
                    self.message = "O leitor encerrou sem resultado.".into();
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
            }
        }
        if !self.open {
            return;
        }
        let mut open = self.open;
        let mut request = None;
        egui::Window::new("Leitor PDF local").open(&mut open).default_size([900.0,700.0]).resizable(true).show(ctx,|ui|{
            if let Some(selection)=&mut self.selection{
                ui.strong(&selection.label);ui.small(format!("Arquivo local #{} • captura #{} • original preservado",selection.file,selection.capture));
                ui.horizontal_wrapped(|ui|{
                    let busy=self.job.is_some();
                    if ui.add_enabled(!busy&&selection.page>0,egui::Button::new("Anterior")).clicked(){selection.page-=1;request=Some(false);}
                    ui.label(format!("Página {} de {}",selection.page+1,selection.pages));
                    if ui.add_enabled(!busy&&selection.page+1<selection.pages,egui::Button::new("Próxima")).clicked(){selection.page+=1;request=Some(false);}
                    if ui.add_enabled(!busy,egui::Button::new("OCR local desta página")).clicked(){request=Some(true);}
                    let previous=selection.width;ui.add_enabled(!busy,egui::Slider::new(&mut selection.width,600..=2200).text("Resolução"));
                    if previous!=selection.width{request=Some(false);}
                });
                ui.small("OCR usa o idioma disponível no perfil Windows. O resultado pode conter erros e não é incorporado automaticamente à análise por IA.");
                if !self.message.is_empty(){ui.label(&self.message);}
                egui::ScrollArea::both().max_height(520.0).show(ui,|ui|{if let Some(texture)=&selection.texture{let size=texture.size_vec2();let ratio=(ui.available_width()/size.x).min(1.0);ui.image((texture.id(),size*ratio));}});
                if let Some(text)=&selection.text{ui.collapsing(format!("Texto OCR salvo • página {} • {}",selection.page+1,text.language),|ui|{ui.small(format!("Obtido localmente em {}",text.created_at));let mut value=text.text.as_str();ui.add(egui::TextEdit::multiline(&mut value).desired_rows(8).desired_width(f32::INFINITY));});}
            }
        });
        self.open = open;
        if let Some(ocr) = request {
            self.request(store, ctx, ocr);
        }
        if !open && self.job.is_none() {
            self.selection = None;
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn native_pdf_renders_fixture_without_export_and_rejects_invalid_page() {
        std::thread::spawn(|| {
            let bytes = include_bytes!("../tests/fixtures/document.pdf");
            let page = super::render(bytes, 0, 800, false).unwrap();
            assert!(page.pages > 0);
            assert_eq!(page.image.size[0], 800);
            assert!(page.image.pixels.iter().any(|p| p.r() < 200));
            assert!(super::render(bytes, 10000, 800, false).is_err());
            assert!(super::render(b"invalid", 0, 800, false).is_err());
        })
        .join()
        .unwrap();
    }
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
    use base64::Engine;
    #[test]
    fn ocr_stays_encrypted_bound_to_file_and_is_removed_with_document() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("ocr.sqlite3");
        let mut store = Store::open(&path).unwrap();
        let mut capture: crate::model::Capture =
            serde_json::from_str(include_str!("../tests/fixtures/capture.json")).unwrap();
        let url="https://www.sigadoc.mt.gov.br/sigaex/app/arquivo/exibir?arquivo=TESTEOFI202600042A.pdf&sigla=TESTEOFI202600042A";
        capture.references.push(crate::model::DocumentReference {
            label: "PDF fictício".into(),
            url: url.into(),
            kind: "original".into(),
        });
        let file = crate::files::FileInput {
            url: url.into(),
            data_base64: base64::engine::general_purpose::STANDARD
                .encode(include_bytes!("../tests/fixtures/document.pdf")),
        };
        let receipt = store.import_bundle(&capture, &[file.clone()]).unwrap();
        let id = store.list_files(receipt.capture_id).unwrap()[0].id;
        let text = PageText {
            text: "OCR_CONFIDENCIAL_TESTE".into(),
            language: "pt-BR".into(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        store.save_page_text(id, 0, &text).unwrap();
        assert_eq!(
            store.load_page_text(id, 0).unwrap().unwrap().text,
            text.text
        );
        assert!(store.load_page_text(id, 1).unwrap().is_none());
        assert!(store.pdf_bytes(id, receipt.capture_id + 1).is_err());
        assert!(store.save_page_text(id, 10000, &text).is_err());
        store
            .conn
            .execute_batch("PRAGMA wal_checkpoint(FULL)")
            .unwrap();
        assert!(!std::fs::read(&path)
            .unwrap()
            .windows(text.text.len())
            .any(|b| b == text.text.as_bytes()));
        assert_eq!(
            store
                .list_documents("OCR_CONFIDENCIAL_TESTE")
                .unwrap()
                .len(),
            1
        );
        capture.request_id = uuid::Uuid::new_v4().to_string();
        capture
            .metadata
            .insert("origin".into(), "Nova captura".into());
        let next = store.import_bundle(&capture, &[file]).unwrap();
        let next_file = store.list_files(next.capture_id).unwrap()[0].id;
        assert_ne!(id, next_file);
        assert_eq!(
            store.load_page_text(next_file, 0).unwrap().unwrap().text,
            text.text
        );
        assert_eq!(
            store
                .list_documents("OCR_CONFIDENCIAL_TESTE")
                .unwrap()
                .len(),
            1
        );
        store.delete_document(receipt.document_id).unwrap();
        assert!(store.load_page_text(id, 0).unwrap().is_none());
    }
}
