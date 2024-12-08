use std::{
    cell::{OnceCell, RefCell},
    collections::HashMap,
    io::Read,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex, OnceLock, RwLock},
};

use chrono::{DateTime, Datelike, Local};
use file_entry::FileEntry;
use flate2::read::GzDecoder;
use tar::Archive;
// use parking_lot::RwLock;
use typst::{
    diag::{EcoString, FileResult, Severity},
    foundations::{Bytes, Datetime},
    layout::Abs,
    model::Document,
    syntax::{package::PackageSpec, FileId, LinkedNode, Source, Span, VirtualPath},
    text::{Font, FontBook},
    utils::LazyHash,
    Library, World,
};
use typst_ide::{analyze_import, Completion, CompletionKind};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

mod fetch;
mod file_entry;

#[wasm_bindgen]
#[derive(Clone)]
pub struct JsSpan {
    span: String,
    file_path: String,
    range: Vec<usize>,
}

#[wasm_bindgen]
impl JsSpan {
    fn from_span(span: Span, sources: HashMap<FileId, FileEntry>) -> Self {
        if span.is_detached() {
            Self {
                span: format!("{:?}", span),
                file_path: String::new(),
                range: Vec::new(),
            }
        } else {
            let file_id = span.id().expect("None detached span should have an id");

            let entry = sources
                .get(&file_id)
                .expect("File should exist because it got compiled");

            let range = entry
                .source
                .range(span)
                .expect("Range should be valid because the span points to the file");

            Self {
                span: format!("{:?}", span),
                file_path: file_id
                    .vpath()
                    .as_rootless_path()
                    .to_str()
                    .unwrap()
                    .to_string(),
                range: Vec::from([range.start, range.end]),
            }
        }
    }

    pub fn get_span(&self) -> String {
        self.span.clone()
    }

    pub fn get_file_path(&self) -> String {
        self.file_path.clone()
    }

    pub fn get_range(&self) -> Vec<usize> {
        self.range.clone()
    }
}

#[wasm_bindgen]
pub struct CompileError {
    severity: Severity,
    message: String,
    root: JsSpan,
    hints: Vec<String>,
    trace: Vec<JsSpan>,
}

#[wasm_bindgen]
impl CompileError {
    fn from_diag(err: typst::diag::SourceDiagnostic, sources: HashMap<FileId, FileEntry>) -> Self {
        let severity = err.severity;
        let message = err.message.to_string();

        let hints = err.hints.iter().map(|hint| hint.to_string()).collect();

        let span = err.span;

        let root = JsSpan::from_span(span, sources.clone());

        let trace = err
            .trace
            .iter()
            .map(|span| JsSpan::from_span(span.span, sources.clone()))
            .collect();

        Self {
            severity,
            message,
            root,
            hints,
            trace,
        }
    }

    pub fn get_severity(&self) -> String {
        match self.severity {
            Severity::Error => "error".to_string(),
            Severity::Warning => "warning".to_string(),
        }
    }

    pub fn get_message(&self) -> String {
        self.message.clone()
    }

    pub fn get_hints(&self) -> Vec<String> {
        self.hints.clone()
    }

    pub fn get_root(&self) -> JsSpan {
        self.root.clone()
    }

    pub fn get_trace(&self) -> Vec<JsSpan> {
        self.trace.clone()
    }
}

#[wasm_bindgen]
pub struct SuiteCore {
    library: OnceLock<LazyHash<Library>>,

    book: OnceLock<LazyHash<FontBook>>,

    sources: Arc<RwLock<HashMap<FileId, FileEntry>>>,

    fonts: Mutex<Vec<Font>>,

    root: PathBuf,

    now: OnceLock<DateTime<Local>>,

    last_doc: Mutex<Option<Document>>,

    packages: RwLock<Vec<PackageSpec>>,
}

struct TypstPackage {
    namespace: String,
    name: String,
    version: String,
}

trait TPFetchable {
    fn fetch(&self) -> HashMap<FileId, FileEntry>;
}

trait TPComparable {
    fn compare(&self, other: &Self) -> bool;
}

impl TPFetchable for PackageSpec {
    fn fetch(&self) -> HashMap<FileId, FileEntry> {
        let path = format!(
            "https://packages.typst.org/preview/{}-{}.tar.gz",
            self.name, self.version
        );
        log(format!("fetching package: {}", path).as_str());
        let fetch = xml_get_sync(path);
        let cursor = std::io::Cursor::new(fetch);
        let gz_decoder = GzDecoder::new(cursor);
        let mut archive = Archive::new(gz_decoder);

        archive
            .entries()
            .unwrap()
            .filter(|entry| {
                entry.is_ok()
                    && entry.as_ref().unwrap().header().entry_type() != tar::EntryType::Directory
            })
            .map(|entry| {
                let mut entry = entry.unwrap();
                let path = entry.path().unwrap().to_string_lossy().into_owned();
                let id = FileId::new(Some(self.clone()), VirtualPath::new(path.clone()));

                // log(format!("extracting: {}, id: {:?}", path, id).as_str()); debug

                let mut content = Vec::new();
                entry.read_to_end(&mut content).unwrap();

                (id, FileEntry::new(id, String::from_utf8(content).unwrap()))
            })
            .collect::<HashMap<FileId, FileEntry>>()
    }
}

impl TPComparable for PackageSpec {
    fn compare(&self, other: &Self) -> bool {
        self.namespace == other.namespace
            && self.name == other.name
            && self.version == other.version
    }
}

#[wasm_bindgen]
#[derive(Copy, Clone)]
pub enum CompletionKindWrapper {
    Syntax,
    Func,
    Type,
    Param,
    Constant,
    Symbol,
}

#[wasm_bindgen]
#[derive(Clone)]
pub struct CompletionKindHighWrapper {
    pub kind: CompletionKindWrapper,
    pub detail: Option<char>,
}

impl From<CompletionKind> for CompletionKindHighWrapper {
    fn from(kind: CompletionKind) -> Self {
        match kind {
            CompletionKind::Syntax => Self {
                kind: CompletionKindWrapper::Syntax,
                detail: None,
            },
            CompletionKind::Func => Self {
                kind: CompletionKindWrapper::Func,
                detail: None,
            },
            CompletionKind::Type => Self {
                kind: CompletionKindWrapper::Type,
                detail: None,
            },
            CompletionKind::Param => Self {
                kind: CompletionKindWrapper::Param,
                detail: None,
            },
            CompletionKind::Constant => Self {
                kind: CompletionKindWrapper::Constant,
                detail: None,
            },
            CompletionKind::Symbol(c) => Self {
                kind: CompletionKindWrapper::Symbol,
                detail: Some(c),
            },
        }
    }
}

#[wasm_bindgen]
pub struct CompletionWrapper {
    kind: CompletionKindHighWrapper,
    label: String,
    apply: Option<String>,
    detail: Option<String>,
}

impl From<Completion> for CompletionWrapper {
    fn from(completion: Completion) -> Self {
        Self {
            kind: completion.kind.into(),
            label: completion.label.to_string(),
            apply: completion.apply.map(|es| es.to_string()),
            detail: completion.detail.map(|es| es.to_string()),
        }
    }
}

#[wasm_bindgen]
impl CompletionWrapper {
    pub fn kind(&self) -> CompletionKindHighWrapper {
        self.kind.clone()
    }

    pub fn label(&self) -> String {
        self.label.clone()
    }

    pub fn apply(&self) -> Option<String> {
        self.apply.clone()
    }

    pub fn detail(&self) -> Option<String> {
        self.detail.clone()
    }
}

#[wasm_bindgen]
extern "C" {
    pub fn xml_get_sync(path: String) -> Vec<u8>;

    pub fn logWasm(s: &str);

    pub fn errorWasm(s: &str);

    // Use `js_namespace` here to bind `console.log(..)` instead of just
    // `log(..)`
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);

    // The `console.log` is quite polymorphic, so we can bind it with multiple
    // signatures. Note that we need to use `js_name` to ensure we always call
    // `log` in JS.
    #[wasm_bindgen(js_namespace = console, js_name = log)]
    fn log_u32(a: u32);

    // Multiple arguments too!
    #[wasm_bindgen(js_namespace = console, js_name = log)]
    fn log_many(a: &str, b: &str);
}

#[wasm_bindgen]
impl SuiteCore {
    #[wasm_bindgen(constructor)]
    pub fn new(root: String) -> Self {
        console_error_panic_hook::set_once();
        let (book, fonts) = Self::start_embedded_fonts();

        let book_lock = OnceLock::new();
        let _ = book_lock.set(LazyHash::new(book)); // TODO: add proper error handling

        Self {
            library: OnceLock::default(),
            book: book_lock,
            sources: Arc::new(RwLock::new(HashMap::new())),
            fonts: Mutex::new(fonts),
            now: OnceLock::default(),
            root: PathBuf::from(root),
            last_doc: Mutex::new(None),
            packages: RwLock::new(Vec::new()),
        }
    }

    // implement packages https://packages.typst.org/preview/index.json
    pub fn autocomplete(
        &self,
        file: String,
        offset: usize,
    ) -> Result<Vec<CompletionWrapper>, JsValue> {
        let source = self
            .source(FileId::new(None, VirtualPath::new(format!("/{}", file))))
            .unwrap();
        let doc = self.last_doc.lock().unwrap().clone();

        match typst_ide::autocomplete(self, doc.as_ref(), &source, offset, true) {
            Some(completions) => Ok(completions.1.into_iter().map(|c| c.into()).collect()),
            None => Ok(Vec::new()),
        }
    }

    fn compile_str(&mut self, text: String) -> Result<String, JsValue> {
        self.reset();

        self.sources.write().unwrap().insert(
            FileId::new(None, VirtualPath::new("/main.typ")),
            FileEntry::new(FileId::new(None, VirtualPath::new("/main.typ")), text),
        );

        match typst::compile(self).output {
            Ok(doc) => Ok(typst_svg::svg(&doc.pages[0])),
            Err(err) => {
                let mut str = String::new();

                for diag in err {
                    match diag.severity {
                        typst::diag::Severity::Error => {
                            str.push_str(format!("error ({:?}): ", diag.span).as_str());
                            str.push_str(&diag.message);
                            str.push_str(format!("{:?}", diag.trace).as_str());
                        }
                        typst::diag::Severity::Warning => {
                            str.push_str(format!("warning ({:?}): ", diag.span).as_str());
                            str.push_str(&diag.message);
                        }
                    }
                    str.push_str("\n\n");
                }

                Err(JsValue::from_str(&str))
            }
        }
    }

    pub fn compile(&mut self, single: bool) -> Result<Vec<String>, Vec<CompileError>> {
        match typst::compile(self).output {
            Ok(doc) => {
                *self.last_doc.lock().unwrap() = Some(doc.clone());
                if single {
                    Ok(vec![typst_svg::svg_merged(&doc, Abs::cm(2.0))])
                } else {
                    Ok(doc.pages.iter().map(typst_svg::svg).collect())
                }
            }
            Err(err) => {
                let mut errs: Vec<CompileError> = Vec::new();

                for diag in err {
                    errs.push(CompileError::from_diag(
                        diag,
                        self.sources.read().unwrap().clone(),
                    ));
                }

                Err(errs)
            }
        }
    }

    pub fn add_file(&mut self, file: String, text: String) -> Result<(), JsValue> {
        let id = FileId::new(None, VirtualPath::new(format!("/{}", file)));
        self.sources
            .write()
            .unwrap()
            .insert(id, FileEntry::new(id, text.clone()));

        Ok(())
    }

    pub fn remove_file(&mut self, file: String) -> Result<(), JsValue> {
        let id = FileId::new(None, VirtualPath::new(format!("/{}", file)));
        self.sources.write().unwrap().remove(&id);

        Ok(())
    }

    pub fn move_file(&mut self, old: String, new: String) -> Result<(), JsValue> {
        let old_id = FileId::new(None, VirtualPath::new(format!("/{}", old)));
        let new_id = FileId::new(None, VirtualPath::new(format!("/{}", new)));

        let entry = self.sources.write().unwrap().remove(&old_id).unwrap();
        self.sources.write().unwrap().insert(new_id, entry);

        Ok(())
    }

    pub fn imports(&self) -> Result<(), JsValue> {
        let res = analyze_import(
            self,
            &LinkedNode::new(self.source(self.main()).unwrap().root()),
        );
        log(format!("imports: {:#?}", res).as_str());
        Ok(())
    }

    pub fn edit(
        &mut self,
        file: String,
        text: String,
        begin: usize,
        end: usize,
    ) -> Result<(), JsValue> {
        let id = FileId::new(None, VirtualPath::new(format!("/{}", file)));
        let mut binding = self.sources.write().unwrap();
        let entry = binding
            .get_mut(&id)
            .ok_or(JsValue::from_str("file not found"))?;

        let range = entry.source.edit(begin..end, text.as_str());

        Ok(())
    }

    fn get_file_entry(&self, id: FileId) -> FileResult<FileEntry> {
        // log(format!("accessing file entry: {:?}", id).as_str()); Debug

        match id.package() {
            Some(package) => {
                if self
                    .packages
                    .read()
                    .unwrap()
                    .iter()
                    .any(|p| package.compare(p))
                {
                    let sources = self.sources.read().unwrap();
                    Ok(sources.get(&id).unwrap().clone())
                } else {
                    /* let path = format!(
                        "https://raw.githubusercontent.com/typst/packages/refs/heads/main/packages/preview/{}/{}/{}",
                        id.package().unwrap().name,
                        id.package().unwrap().version,
                        id.vpath().as_rootless_path().to_str().unwrap()
                    ); */
                    let fetched_sources = package.fetch();
                    {
                        let mut writer = self.sources.write().unwrap();
                        for (id, entry) in fetched_sources.iter() {
                            writer.insert(*id, entry.clone());
                        }
                    }
                    self.packages.write().unwrap().push(package.clone());
                    if fetched_sources.contains_key(&id) {
                        Ok(fetched_sources.get(&id).unwrap().clone())
                    } else {
                        Err(typst::diag::FileError::NotSource)
                    }
                }
            }
            None => {
                let sources = self.sources.read().unwrap();
                match sources.get(&id) {
                    Some(entry) => Ok(entry.clone()),
                    None => Err(typst::diag::FileError::NotSource),
                }
            }
        }
    }
}

impl World for SuiteCore {
    fn library(&self) -> &LazyHash<Library> {
        self.library
            .get_or_init(|| LazyHash::new(Library::builder().build()))
    }

    fn book(&self) -> &LazyHash<FontBook> {
        self.book.get_or_init(|| todo!())
    }

    fn main(&self) -> FileId {
        FileId::new(None, VirtualPath::new("/main.typ"))
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        self.get_file_entry(id).map(|entry| entry.source)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.get_file_entry(id).map(|entry| entry.bytes())
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.lock().unwrap().get(index).cloned()
    }

    fn today(&self, offset: Option<i64>) -> Option<Datetime> {
        let now = self.now.get_or_init(chrono::Local::now);

        let naive = match offset {
            None => now.naive_local(),
            Some(o) => now.naive_utc() + chrono::Duration::hours(o),
        };

        Datetime::from_ymd(
            naive.year(),
            naive.month().try_into().ok()?,
            naive.day().try_into().ok()?,
        )
    }

    // TODO: implement packages()
}

impl SuiteCore {
    fn reset(&mut self) {
        self.library = OnceLock::default();
        self.sources = Arc::new(RwLock::new(HashMap::new()));
        self.now = OnceLock::default();
    }

    fn start_embedded_fonts() -> (FontBook, Vec<Font>) {
        let mut book = FontBook::new();
        let mut fonts = Vec::new();

        for data in typst_assets::fonts() {
            let buffer = Bytes::from_static(data);
            for font in Font::iter(buffer) {
                book.push(font.info().clone());
                fonts.push(font);
            }
        }

        (book, fonts)
    }
}

#[wasm_bindgen]
pub fn greet(name: &str) -> String {
    format!("Hello, {}!", name)
}
