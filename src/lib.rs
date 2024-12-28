use std::{
    collections::HashMap,
    io::Read,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex, OnceLock, RwLock},
};

use chrono::{format, DateTime, Datelike, Local};
use file_entry::FileEntry;
use flate2::read::GzDecoder;
use js_types::RawPackageSpec;
use tar::Archive;
// use parking_lot::RwLock;
use typst::{
    diag::{EcoString, FileResult},
    foundations::{Bytes, Datetime},
    layout::Abs,
    model::Document,
    syntax::{
        package::{PackageSpec, PackageVersion},
        FileId, LinkedNode, Source, VirtualPath,
    },
    text::{Font, FontBook},
    utils::LazyHash,
    Library, World,
};
use typst_ide::{analyze_import, tooltip};
use wasm_bindgen::prelude::*;

mod fetch;
mod file_entry;
mod js_types;

#[wasm_bindgen]
pub struct SuiteCore {
    library: OnceLock<LazyHash<Library>>,

    book: OnceLock<LazyHash<FontBook>>,

    sources: Arc<RwLock<HashMap<FileId, FileEntry>>>,

    fonts: Mutex<Vec<Font>>,

    root: PathBuf,

    now: OnceLock<DateTime<Local>>,

    last_doc: Mutex<Option<Document>>,

    packages: RwLock<Vec<PackageWrapper>>,

    package_index: OnceLock<Vec<(PackageSpec, Option<EcoString>)>>,
}

#[derive(Clone, Debug)]
enum ExtendedPackageVersion {
    Latest,
    Version(PackageVersion),
}

impl ExtendedPackageVersion {
    fn from_str(version: &str) -> Result<Self, String> {
        if version == "latest" {
            Ok(Self::Latest)
        } else {
            let version = PackageVersion::from_str(version).map_err(|e| e.to_string())?;
            Ok(Self::Version(version))
        }
    }

    fn version(&self) -> &PackageVersion {
        match self {
            Self::Latest => &PackageVersion {
                major: 0,
                minor: 0,
                patch: 0,
            },
            Self::Version(v) => v,
        }
    }
}

impl From<PackageVersion> for ExtendedPackageVersion {
    fn from(version: PackageVersion) -> Self {
        Self::Version(version)
    }
}

impl PartialEq for ExtendedPackageVersion {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Latest, Self::Latest) => true,
            (Self::Version(v1), Self::Version(v2)) => v1 == v2,
            _ => false,
        }
    }
}

impl From<RawPackageSpec> for PackageWrapper {
    fn from(spec: RawPackageSpec) -> Self {
        Self {
            namespace: EcoString::from(spec.namespace),
            name: EcoString::from(spec.name),
            version: ExtendedPackageVersion::from_str(spec.version.as_str()).unwrap(),
            fetched: false,
            description: spec.description.map(EcoString::from),
        }
    }
}

#[derive(Clone, Debug)]
struct PackageWrapper {
    namespace: EcoString,
    name: EcoString,
    version: ExtendedPackageVersion,
    fetched: bool,
    description: Option<EcoString>,
}

impl PackageWrapper {
    fn to_string(&self) -> String {
        format!(
            "{}-{}-{}/{}",
            self.namespace,
            self.name,
            match &self.version {
                ExtendedPackageVersion::Latest => "latest".to_string(),
                ExtendedPackageVersion::Version(v) => v.to_string(),
            },
            self.fetched
        )
    }
}

impl From<PackageSpec> for PackageWrapper {
    fn from(spec: PackageSpec) -> Self {
        Self {
            namespace: spec.namespace,
            name: spec.name,
            version: ExtendedPackageVersion::from(spec.version),
            fetched: false,
            description: None,
        }
    }
}

impl From<PackageWrapper> for PackageSpec {
    fn from(wrapper: PackageWrapper) -> Self {
        Self {
            namespace: wrapper.namespace,
            name: wrapper.name,
            version: *wrapper.version.version(),
        }
    }
}

trait TPFetchable {
    fn fetch(&self) -> HashMap<FileId, FileEntry>;
}

impl TPFetchable for PackageSpec {
    fn fetch(&self) -> HashMap<FileId, FileEntry> {
        let path = {
            if self.namespace().starts_with("wolframe-") {
                let args = self.namespace().split("-").collect::<Vec<&str>>();
                format!(
                    "/packages/download?uname={}&pname={}",
                    args[1..].join("-"),
                    self.name()
                )
            } else {
                format!(
                    "https://packages.typst.org/preview/{}-{}.tar.gz",
                    self.name, self.version
                )
            }
        };
        log(format!(
            "fetching package: {}, {:?}, {}, {}",
            path,
            self,
            self.namespace().starts_with("@wolframe-"),
            self.namespace()
        )
        .as_str());
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

trait UnifiedPackageSpec {
    fn namespace(&self) -> &EcoString;
    fn name(&self) -> &EcoString;
    fn version(&self) -> &PackageVersion;
}

impl UnifiedPackageSpec for PackageSpec {
    fn namespace(&self) -> &EcoString {
        &self.namespace
    }

    fn name(&self) -> &EcoString {
        &self.name
    }

    fn version(&self) -> &PackageVersion {
        &self.version
    }
}

impl UnifiedPackageSpec for PackageWrapper {
    fn namespace(&self) -> &EcoString {
        &self.namespace
    }

    fn name(&self) -> &EcoString {
        &self.name
    }

    fn version(&self) -> &PackageVersion {
        self.version.version()
    }
}

trait TPComparable {
    fn compare<R>(&self, other: &R) -> bool
    where
        R: UnifiedPackageSpec;
}

impl<T> TPComparable for T
where
    T: UnifiedPackageSpec,
{
    fn compare<R>(&self, other: &R) -> bool
    where
        R: UnifiedPackageSpec,
    {
        self.namespace() == other.namespace()
            && self.name() == other.name()
            && self.version() == other.version()
    }
}

#[wasm_bindgen]
extern "C" {
    pub fn xml_get_sync(path: String) -> Vec<u8>;

    pub fn logWasm(s: &str);

    #[wasm_bindgen(js_name = logWasm)]
    pub fn log_wasm_any(s: Vec<String>);

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
            package_index: OnceLock::default(),
        }
    }

    pub fn get_files(&self) -> Vec<String> {
        self.sources
            .read()
            .unwrap()
            .keys()
            .map(|id| id.vpath().as_rootless_path().to_str().unwrap().to_string())
            .collect()
    }

    pub fn set_root(&mut self, root: String) -> Result<(), String> {
        // test for valid path (path in sources)
        if !self
            .sources
            .read()
            .unwrap()
            .keys()
            .any(|id| id.vpath().as_rootless_path().to_str().unwrap() == root.as_str())
        {
            return Err("The provided root path is not valid.".to_string());
        }

        self.root = PathBuf::from(root);
        Ok(())
    }

    pub fn add_packages(&mut self, packages: Vec<RawPackageSpec>) {
        let mut lock = self.packages.write().unwrap();
        for package in packages {
            lock.push(package.into());
        }
    }

    // implement packages https://packages.typst.org/preview/index.json
    pub fn autocomplete(
        &self,
        file: String,
        offset: usize,
    ) -> Result<Vec<js_types::Completion>, JsValue> {
        let source = self
            .source(FileId::new(None, VirtualPath::new(&file)))
            .map_err(|e| JsValue::from_str(&format!("{:?}", e)))?;

        let doc = self.last_doc.lock().unwrap().clone();

        match typst_ide::autocomplete(self, doc.as_ref(), &source, offset, true) {
            Some(completions) => Ok(completions.1.into_iter().map(|c| c.into()).collect()),
            None => Ok(Vec::new()),
        }
    }

    pub fn definition(&self, file: String, offset: usize) -> Result<Option<js_types::Definition>, JsValue> {
        let source = self
            .source(FileId::new(None, VirtualPath::new(&file)))
            .map_err(|e| JsValue::from_str(&format!("{:?}", e)))?;

        let doc = self.last_doc.lock().unwrap().clone();

        logWasm(
            format!(
                "Tooltip: {:?}", tooltip(self, doc.as_ref(), &source, offset, typst::syntax::Side::After)
            ).as_str()
        );

        Ok(typst_ide::definition(self, doc.as_ref(), &source, offset, typst::syntax::Side::After).map(|def| js_types::Definition::new(def, &source)))
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

    pub fn compile(&mut self, single: bool) -> Result<Vec<String>, Vec<js_types::Diagnostics>> {
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
                let mut errs: Vec<js_types::Diagnostics> = Vec::new();

                for diag in err {
                    errs.push(js_types::Diagnostics::from_diag(
                        diag,
                        self.sources.read().unwrap().clone(),
                    ));
                }

                Err(errs)
            }
        }
    }

    pub fn add_file(&mut self, file: String, text: String) -> Result<(), JsValue> {
        let id = FileId::new(None, VirtualPath::new(&file));
        self.sources
            .write()
            .unwrap()
            .insert(id, FileEntry::new(id, text.clone()));

        Ok(())
    }

    pub fn remove_file(&mut self, file: String) -> Result<(), JsValue> {
        let id = FileId::new(None, VirtualPath::new(&file));
        self.sources.write().unwrap().remove(&id);

        Ok(())
    }

    pub fn move_file(&mut self, old: String, new: String) -> Result<(), JsValue> {
        let old_id = FileId::new(None, VirtualPath::new(&old));
        let new_id = FileId::new(None, VirtualPath::new(&new));

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
        let id = FileId::new(None, VirtualPath::new(&file));
        let mut binding = self.sources.write().unwrap();
        let entry = binding
            .get_mut(&id)
            .ok_or(JsValue::from_str("file not found"))?;

        let range = entry.source.edit(begin..end, text.as_str());

        Ok(())
    }

    fn get_file_entry(&self, id: FileId) -> FileResult<FileEntry> {
        // log(format!("accessing file entry: {:?}", id).as_str()); Debug

        logWasm(
            format!(
                "accessing file entry: {:?}, package: {:?}",
                id,
                id.package()
            )
            .as_str(),
        );

        match id.package() {
            Some(package) => {
                let mut lock = self.packages.write().unwrap();
                let int_package_opt: Option<&mut PackageWrapper> =
                    lock.iter_mut().find(|p| package.compare(*p));

                if int_package_opt.is_none() {
                    return Err(typst::diag::FileError::NotFound(
                        id.vpath().as_rootless_path().to_path_buf(),
                    ));
                }

                let int_package = int_package_opt.unwrap();

                if int_package.fetched
                    && !(int_package.namespace().starts_with("wolframe-")
                        && int_package.version
                            == ExtendedPackageVersion::from_str("latest").unwrap())
                {
                    logWasm(
                        format!(
                            "package already fetched: {:?}, {}, {}, {}",
                            id,
                            int_package.namespace().starts_with("wolframe-"),
                            (int_package.version
                                == ExtendedPackageVersion::from_str("latest").unwrap()),
                            !(int_package.namespace().starts_with("wolframe-")
                                && (int_package.version
                                    == ExtendedPackageVersion::from_str("latest").unwrap()))
                        )
                        .as_str(),
                    );
                    let sources = self.sources.read().unwrap();
                    Ok(sources.get(&id).unwrap().clone())
                } else {
                    logWasm(format!("fetching package: {:?}", id).as_str());
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
                    int_package.fetched = true;
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
        FileId::new(None, VirtualPath::new(&self.root))
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
    fn packages(&self) -> &[(PackageSpec, Option<EcoString>)] {
        self.package_index.get_or_init(|| {
            let lock = self.packages.read().unwrap();
            lock.iter()
                .map(|p| (p.clone().into(), p.description.clone()))
                .collect()
        })
    }
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
