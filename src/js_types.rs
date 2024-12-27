use std::collections::HashMap;

use serde::Serialize;
use typst::syntax::{FileId, Source, Span};
use wasm_bindgen::{prelude::wasm_bindgen, JsValue};

use crate::file_entry::FileEntry;

#[macro_export]
macro_rules! define_lowercase_enum {
    ($vis:vis enum $name:ident { $($variant:ident),+ $(,)? }) => {
        #[wasm_bindgen]
        #[derive(Copy, Clone)]
        $vis enum $name {
            $($variant = stringify!($variant).to_lowercase().as_str()),+
        }
    }
}

#[wasm_bindgen(getter_with_clone)]
#[derive(Clone, Debug, Serialize)]
pub struct ResolvedSpan {
    pub span: String,
    pub file_path: String,
    pub start_offset: usize,
    pub end_offset: usize,
}

impl ResolvedSpan {
    pub fn from_source(span: Span, source: &Source) -> Self {
        if span.is_detached() {
            Self {
                span: format!("{:?}", span),
                file_path: String::new(),
                start_offset: 0,
                end_offset: 0,
            }
        } else {
            let range = source
                .range(span)
                .expect("Range should be valid because the span points to the file");

            Self {
                span: format!("{:?}", span),
                file_path: source.id().vpath().as_rooted_path().to_str().unwrap().to_string(),
                start_offset: range.start, 
                end_offset: range.end,
            }
        }
    }

    pub fn from_sources(span: Span, sources: HashMap<FileId, FileEntry>) -> Self {
        if span.is_detached() {
            Self {
                span: format!("{:?}", span),
                file_path: String::new(),
                start_offset: 0,
                end_offset: 0,
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
                    .as_rooted_path()
                    .to_str()
                    .unwrap()
                    .to_string(),
                start_offset: range.start, 
                end_offset: range.end,
            }
        }
    }
}


/* 
 * Diagnostics
 */

#[wasm_bindgen]
#[derive(Copy, Clone, Serialize)]
pub enum Severity {
    Error = "error",
    Warning = "warning",
}

impl From<typst::diag::Severity> for Severity {
    fn from(severity: typst::diag::Severity) -> Self {
        match severity {
            typst::diag::Severity::Error => Self::Error,
            typst::diag::Severity::Warning => Self::Warning,
        }
    }
}

#[wasm_bindgen(getter_with_clone)]
#[derive(Serialize)]
pub struct Diagnostics {
    pub severity: Severity,
    pub message: String,
    pub root: ResolvedSpan,
    pub hints: Vec<String>,
    pub trace: Vec<ResolvedSpan>,
}

#[wasm_bindgen]
impl Diagnostics {
    pub fn to_json(&self) -> JsValue {
        serde_wasm_bindgen::to_value(self).unwrap()
    }
}

impl Diagnostics {
    pub fn from_diag(err: typst::diag::SourceDiagnostic, sources: HashMap<FileId, FileEntry>) -> Self {
        let severity = Severity::from(err.severity);
        let message = err.message.to_string();

        let hints = err.hints.iter().map(|hint| hint.to_string()).collect();

        let span = err.span;

        let root = ResolvedSpan::from_sources(span, sources.clone());

        let trace = err
            .trace
            .iter()
            .map(|span| ResolvedSpan::from_sources(span.span, sources.clone()))
            .collect();

        Self {
            severity,
            message,
            root,
            hints,
            trace,
        }
    }
}


/* 
 * Completion
 */

#[wasm_bindgen]
#[derive(Copy, Clone, Serialize)]
pub enum CompletionKind {
    Syntax = "syntax",
    Func = "func",
    Type = "type",
    Param = "param",
    Constant = "constant",
    Symbol = "symbol",
}

#[wasm_bindgen]
#[derive(Copy, Clone, Serialize)]
pub struct CompletionDetail {
    pub kind: CompletionKind,
    pub detail: Option<char>,
}

impl From<typst_ide::CompletionKind> for CompletionDetail {
    fn from(kind: typst_ide::CompletionKind) -> Self {
        match kind {
            typst_ide::CompletionKind::Syntax => Self {
                kind: CompletionKind::Syntax,
                detail: None,
            },
            typst_ide::CompletionKind::Func => Self {
                kind: CompletionKind::Func,
                detail: None,
            },
            typst_ide::CompletionKind::Type => Self {
                kind: CompletionKind::Type,
                detail: None,
            },
            typst_ide::CompletionKind::Param => Self {
                kind: CompletionKind::Param,
                detail: None,
            },
            typst_ide::CompletionKind::Constant => Self {
                kind: CompletionKind::Constant,
                detail: None,
            },
            typst_ide::CompletionKind::Symbol(c) => Self {
                kind: CompletionKind::Symbol,
                detail: Some(c),
            },
        }
    }
}

#[wasm_bindgen(getter_with_clone)]
#[derive(Serialize)]
pub struct Completion {
    pub kind: CompletionDetail,
    pub label: String,
    pub apply: Option<String>,
    pub detail: Option<String>,
}

#[wasm_bindgen]
impl Completion {
    pub fn to_json(&self) -> JsValue {
        serde_wasm_bindgen::to_value(self).unwrap()
    }
}

impl From<typst_ide::Completion> for Completion {
    fn from(completion: typst_ide::Completion) -> Self {
        Self {
            kind: completion.kind.into(),
            label: completion.label.to_string(),
            apply: completion.apply.map(|es| es.to_string()),
            detail: completion.detail.map(|es| es.to_string()),
        }
    }
}


/* 
 * Definition
 */

#[wasm_bindgen(getter_with_clone)]
#[derive(Clone, Serialize)]
pub struct Value {
    pub display: String,
    pub name: Option<String>,
    pub docs: Option<String>,
}

impl From<typst::foundations::Value> for Value {
    fn from(value: typst::foundations::Value) -> Self {
        Self {
            name: value.name().map(|name| name.to_string()),
            docs: value.docs().map(|docs| docs.to_string()),
            display: value.display().plain_text().to_string(),
        }
    }
}

#[wasm_bindgen]
#[derive(Clone, Copy, Serialize)]
pub enum DefinitionKind {
    Variable = "variable",
    Function = "function",
    Module = "module",
    Label = "label",
}

impl From<typst_ide::DefinitionKind> for DefinitionKind {
    fn from(kind: typst_ide::DefinitionKind) -> Self {
        match kind {
            typst_ide::DefinitionKind::Variable => DefinitionKind::Variable,
            typst_ide::DefinitionKind::Function => DefinitionKind::Function,
            typst_ide::DefinitionKind::Module => DefinitionKind::Module,
            typst_ide::DefinitionKind::Label => DefinitionKind::Label,
        }
    }
}

#[wasm_bindgen(getter_with_clone)]
#[derive(Serialize)]
pub struct Definition {
    pub name: String,
    pub span: ResolvedSpan,
    pub name_span: ResolvedSpan,
    pub kind: DefinitionKind,
    pub value: Option<Value>,
}

#[wasm_bindgen]
impl Definition {
    pub fn to_json(&self) -> JsValue {
        serde_wasm_bindgen::to_value(self).unwrap()
    }
}

impl Definition {
    pub fn new(definition: typst_ide::Definition, source: &Source) -> Self {
        let name = definition.name.to_string();
        let span = ResolvedSpan::from_source(definition.span, source);
        let name_span = ResolvedSpan::from_source(definition.name_span, source);
        let kind = DefinitionKind::from(definition.kind);
        let value = definition.value.map(Value::from);

        Self {
            name,
            span,
            name_span,
            kind,
            value,
        }
    }
}

/* 
 * Package Spec
 */

#[wasm_bindgen(getter_with_clone)]
#[derive(Clone, Serialize)]
pub struct RawPackageSpec {
    pub namespace: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
}

#[wasm_bindgen]
impl RawPackageSpec {
    #[wasm_bindgen(constructor)]
    pub fn new(
        namespace: String,
        name: String,
        version: String,
        description: Option<String>,
    ) -> Self {
        Self {
            namespace,
            name,
            version,
            description,
        }
    }

    pub fn to_json(&self) -> JsValue {
        serde_wasm_bindgen::to_value(self).unwrap()
    }
}

