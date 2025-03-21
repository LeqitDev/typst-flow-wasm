use std::collections::HashMap;

use chrono::offset;
use serde::Serialize;
use typst::syntax::{FileId, LinkedNode, Source, Span, SyntaxKind};
use wasm_bindgen::{prelude::wasm_bindgen, JsValue};

use crate::{
    file_entry::FileEntry,
    logWasm,
    tidy::{collect_tidy_doc, parse_doc_str},
};

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
                .expect("Range should point to the source file. Looks like it does not.");

            Self {
                span: format!("{:?}", span),
                file_path: source
                    .id()
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

    pub fn from_sources(span: Span, sources: &HashMap<FileId, FileEntry>) -> Self {
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

            let source = entry.source();

            let range = source
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
    pub fn from_diag(
        err: typst::diag::SourceDiagnostic,
        sources: HashMap<FileId, FileEntry>,
    ) -> Self {
        let severity = Severity::from(err.severity);
        let message = err.message.to_string();

        let hints = err.hints.iter().map(|hint| hint.to_string()).collect();

        let span = err.span;

        let root = ResolvedSpan::from_sources(span, &sources);

        let trace = err
            .trace
            .iter()
            .map(|span| ResolvedSpan::from_sources(span.span, &sources))
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
#[derive(Serialize, Clone)]
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
    pub fn new(definition: typst_ide::Definition, sources: HashMap<FileId, FileEntry>) -> Self {
        let name = definition.name.to_string();
        let span = ResolvedSpan::from_sources(definition.span, &sources);
        let name_span = ResolvedSpan::from_sources(definition.name_span, &sources);
        let kind = DefinitionKind::from(definition.kind.clone());
        let mut value = definition.value.map(Value::from);

        if (value.is_none() || value.as_ref().unwrap().docs.is_none())
            && (definition.kind == typst_ide::DefinitionKind::Function
                || definition.kind == typst_ide::DefinitionKind::Variable)
        {
            let target = definition.name_span;

            if !target.is_detached() {
                let file_id = target.id().expect("None detached span should have an id");

                let entry = sources
                    .get(&file_id)
                    .expect("File should exist because it got compiled");

                let source = entry.source();

                let node = source.find(target).unwrap().parent().unwrap().clone();
                let collected = collect_tidy_doc(node);

                logWasm(&format!("Collected: {:?}", collected));

                value = Some(Value {
                    name: Some(name.clone()),
                    display: name.clone(),
                    docs: Some(
                        parse_doc_str(definition.name.to_string(), collected).to_doc_string(),
                    ),
                });
            }
        }

        Self {
            name,
            span,
            name_span,
            kind,
            value,
        }
    }
}

#[wasm_bindgen(getter_with_clone)]
#[derive(Serialize, Clone)]
pub struct Tooltip {
    pub code: Option<String>,
    pub text: Option<String>,
}

#[wasm_bindgen]
impl Tooltip {
    pub fn to_json(&self) -> JsValue {
        serde_wasm_bindgen::to_value(self).unwrap()
    }
}

impl Tooltip {
    pub fn new(tooltip: typst_ide::Tooltip) -> Self {
        match tooltip {
            typst_ide::Tooltip::Code(code) => Self {
                code: Some(code.to_string()),
                text: None,
            },
            typst_ide::Tooltip::Text(text) => Self {
                code: None,
                text: Some(text.to_string()),
            },
        }
    }
}

#[wasm_bindgen(getter_with_clone)]
#[derive(Serialize)]
pub struct HoverProvider {
    pub definition: Option<Definition>,
    pub tooltip: Option<Tooltip>,
}

#[wasm_bindgen]
impl HoverProvider {
    pub fn to_json(&self) -> JsValue {
        serde_wasm_bindgen::to_value(self).unwrap()
    }
}

impl HoverProvider {
    pub fn new(definition: Option<Definition>, tooltip: Option<Tooltip>) -> Self {
        Self {
            definition,
            tooltip,
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

#[wasm_bindgen(getter_with_clone)]
#[derive(Clone, Serialize)]
pub struct AstNode {
    pub raw: String,
    pub children: Vec<AstNode>,
    pub index: usize,
    pub offset: usize,
    pub kind: String,
}

impl AstNode {
    pub fn from_node(node: LinkedNode) -> Self {
        let offset = node.offset();
        let index = node.index();
        let raw = node.text().to_string();
        let children = node
            .children()
            .map(|child| Self::from_node(child.clone()))
            .collect();
        let kind = node.kind().name().to_string();

        Self {
            raw,
            children,
            index,
            offset,
            kind,
        }
    }

    pub fn from_source(source: Source) -> Self {
        let raw_root = source.root();

        Self::from_node(LinkedNode::new(raw_root))
    }
}

#[wasm_bindgen]
impl AstNode {
    pub fn to_json(&self) -> JsValue {
        serde_wasm_bindgen::to_value(self).unwrap()
    }
}

/*
 * Tidy Docs
 */

#[wasm_bindgen]
#[derive(Clone, Serialize, Debug)]
pub enum TidyType {
    Function,
    Variable,
}

#[derive(Clone, Serialize, Debug)]
pub struct TidyComments {
    pub pre: String,     // Comments before the function/variable
    pub type_: TidyType, // The type of the function/variable
    pub args: Vec<(String, String, Option<String>)>, // Comments before the arguments of th function
}

impl TidyComments {
    pub fn new(pre: String) -> Self {
        Self {
            pre,
            type_: TidyType::Function,
            args: Vec::new(),
        }
    }

    pub fn add_arg(&mut self, arg: String, doc: String, default: Option<String>) {
        self.args.push((arg, doc, default));
    }

    pub fn set_type(&mut self, type_: TidyType) {
        self.type_ = type_;
    }

    pub fn has_args(&self) -> bool {
        !self.args.is_empty()
    }
}

#[wasm_bindgen(getter_with_clone)]
#[derive(Clone, Serialize)]
pub struct TidyDocs {
    pub name: String,
    pub type_: TidyType,
    pub description: Option<String>,
    pub return_types: Vec<String>,
    pub arguments: Vec<TidyArgDocs>,
}

impl TidyDocs {
    pub fn new(name: String, type_: TidyType) -> Self {
        Self {
            name,
            type_,
            description: None,
            return_types: Vec::new(),
            arguments: Vec::new(),
        }
    }

    pub fn add_description(&mut self, description: String) {
        self.description = Some(description);
    }

    pub fn add_return_type(&mut self, return_type: String) {
        self.return_types.push(return_type);
    }

    pub fn add_argument(&mut self, arg: TidyArgDocs) {
        self.arguments.push(arg);
    }

    pub fn to_doc_string(&self) -> String {
        let mut result = String::new();

        fn format_types(types: &[String], join: &str) -> String {
            types
                .iter()
                .map(|t| format!("<div data-code=\"type\">{}</div>", t))
                .collect::<Vec<String>>()
                .join(join)
        }

        result.push_str("<div data-code=\"desc\">");
        if let Some(description) = &self.description {
            result.push_str(&format!(
                "<div data-code=\"text\">{}</div>",
                description.replace("\n", "")
            ));
        }

        if !self.arguments.is_empty() {
            result.push_str("<div data-code=\"h1\">Parameters</div>");
            let mut arg_str = String::new();
            for (i, arg) in self.arguments.iter().enumerate() {
                let types = if arg.types.is_empty() {
                    "".to_string()
                } else {
                    format!(": {}", format_types(&arg.types, " "))
                };
                arg_str.push_str(&format!(
                    "<div data-code=\"function-arg\">{}{}{}{}</div>",
                    arg.name,
                    if arg.default.is_some() { "?" } else { "" },
                    types,
                    if i < self.arguments.len() - 1 {
                        ", "
                    } else {
                        ""
                    }
                ));
            }
            let return_types = if self.return_types.is_empty() {
                "".to_string()
            } else {
                format!(" -> {}", format_types(&self.return_types, " "))
            };

            result.push_str(&format!(
                "<div data-code=\"function\"><div data-code=\"name\">{}</div>({}){}</div>",
                self.name,
                arg_str.trim_end_matches(", "),
                return_types
            ));

            for arg in &self.arguments {
                result.push_str("<div data-code=\"arg\">");
                result.push_str(&format!(
                    "<div data-code=\"arg-heading\"><div data-code=\"arg-name\">{}</div> {}</div>",
                    arg.name,
                    format_types(&arg.types, " <div data-code=\"or\">or</div> ")
                ));
                if let Some(description) = &arg.description {
                    result.push_str(&format!(
                        "<div data-code=\"arg-content\">{}</div><div data-code=\"arg-default\">{}</div>",
                        description,
                        if arg.default.is_some() {
                            format!("Default: {}", arg.default.as_ref().unwrap())
                        } else {
                            "".to_string()
                        }
                    ));
                }
                result.push_str("</div>");
            }
        } else {
            let return_types = if self.return_types.is_empty() {
                "".to_string()
            } else {
                format!(" -> {}", format_types(&self.return_types, " "))
            };
            result.push_str(&format!(
                "<div data-code=\"function\"><div data-code=\"name\">{}</div>{}{}</div>",
                self.name,
                match self.type_ {
                    TidyType::Function => "()",
                    TidyType::Variable => "",
                },
                return_types
            ));
        }
        result.push_str("</>");

        result
    }
}

#[wasm_bindgen]
impl TidyDocs {
    pub fn to_json(&self) -> JsValue {
        serde_wasm_bindgen::to_value(self).unwrap()
    }
}

#[wasm_bindgen(getter_with_clone)]
#[derive(Clone, Serialize)]
pub struct TidyArgDocs {
    pub name: String,
    pub types: Vec<String>,
    pub description: Option<String>,
    pub default: Option<String>,
}

impl TidyArgDocs {
    pub fn new(name: String) -> Self {
        Self {
            name,
            types: Vec::new(),
            description: None,
            default: None,
        }
    }

    pub fn add_type(&mut self, type_: String) {
        self.types.push(type_);
    }

    pub fn add_description(&mut self, description: String) {
        self.description = Some(description);
    }

    pub fn add_default(&mut self, default: String) {
        self.default = Some(default);
    }
}

#[wasm_bindgen]
impl TidyArgDocs {
    pub fn to_json(&self) -> JsValue {
        serde_wasm_bindgen::to_value(self).unwrap()
    }
}
