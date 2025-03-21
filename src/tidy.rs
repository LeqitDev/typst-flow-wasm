use typst::syntax::LinkedNode;

use crate::{
    ast::{get_args, get_prev, get_prev_kind, is_function, is_variable},
    js_types, logWasm,
};

fn parse_description(description: String) -> String {
    description
        .lines()
        .filter(|s| s.starts_with("///"))
        .map(|s| s.replace("///", "").trim().to_string())
        .collect::<Vec<String>>()
        .join("\n")
        .trim()
        .to_string()
}

pub fn parse_doc_str(name: String, doc: js_types::TidyComments) -> js_types::TidyDocs {
    let mut docs = js_types::TidyDocs::new(name, doc.type_);

    // description
    let (description, return_types) = {
        if doc.pre.contains("->") {
            let mut parts = doc.pre.split("->");
            let description = parse_description(parts.next().unwrap().to_string());
            let return_types: Vec<&str> = parts
                .next()
                .unwrap()
                .trim()
                .split("|")
                .map(|s| s.trim())
                .collect();
            (description, Some(return_types))
        } else {
            (parse_description(doc.pre.to_string()), None)
        }
    };
    docs.add_description(description);
    if return_types.is_some() {
        for return_type in return_types.unwrap() {
            docs.add_return_type(return_type.to_string());
        }
    }

    // params
    for (name, comments, default) in doc.args {
        let mut param = js_types::TidyArgDocs::new(name);
        if comments.contains("->") {
            let mut parts = comments.split("->");
            let description = parse_description(parts.next().unwrap().to_string());
            let return_types: Vec<&str> = parts
                .next()
                .unwrap()
                .trim()
                .split("|")
                .map(|s| s.trim())
                .collect();
            param.add_description(description);
            for return_type in return_types {
                param.add_type(return_type.to_string());
            }
        } else {
            param.add_description(parse_description(comments));
        }
        if let Some(default) = default {
            param.add_default(default);
        }
        docs.add_argument(param);
    }

    docs
}

pub fn collect_tidy_doc(mut node: LinkedNode<'_>) -> js_types::TidyComments {
    let origin = node.clone();
    // Walk backwards until the first Space node starting with "\r\n"
    while let Some(prev) = get_prev(&node) {
        if prev.kind() == typst::syntax::SyntaxKind::Space && prev.text().starts_with("\r\n") {
            break;
        }
        node = prev;
    }

    let mut lines = Vec::new();
    // walk back to the first non Linecomment line after '\r\n'
    while let Some(prev) = get_prev(&node) {
        if prev.kind() == typst::syntax::SyntaxKind::Space && prev.text().starts_with("\r\n") {
            node = prev;
            if let Some(prev_comment) = get_prev(&node) {
                let prev_kind = get_prev_kind(&prev_comment);
                if prev_comment.kind() == typst::syntax::SyntaxKind::LineComment
                    && (prev_kind == Some(typst::syntax::SyntaxKind::Space)
                        || prev_kind == Some(typst::syntax::SyntaxKind::Parbreak)
                        || prev_kind.is_none())
                    && prev_comment.text().starts_with("///")
                {
                    lines.push(prev_comment.text().trim().to_string());
                    node = prev_comment;
                    continue;
                }
            }
        }
        break;
    }

    lines.reverse();
    let mut tidy = js_types::TidyComments::new(lines.join("\n"));

    if !is_function(&node) && is_variable(&node) {
        tidy.set_type(js_types::TidyType::Variable);
    }

    if let Some(args) = get_args(&origin) {
        for arg in args {
            let mut default = None;
            let name = match arg.kind() {
                typst::syntax::SyntaxKind::Ident => arg.text().trim().to_string(),
                typst::syntax::SyntaxKind::Named => {
                    let name = arg
                        .children()
                        .find(|n| n.kind() == typst::syntax::SyntaxKind::Ident);
                    default = Some(arg.children().last().unwrap().text().trim().to_string());
                    if let Some(name) = name {
                        name.text().trim().to_string()
                    } else {
                        String::new()
                    }
                }
                _ => String::new(),
            };

            lines.clear();

            let mut node = arg;

            while let Some(prev) = get_prev(&node) {
                if prev.kind() == typst::syntax::SyntaxKind::Space
                    && prev.text().starts_with("\r\n")
                {
                    node = prev;
                    if let Some(prev_comment) = get_prev(&node) {
                        let prev_kind = get_prev_kind(&prev_comment);
                        if prev_comment.kind() == typst::syntax::SyntaxKind::LineComment
                            && (prev_kind == Some(typst::syntax::SyntaxKind::Space)
                                || prev_kind.is_none())
                            && prev_comment.text().starts_with("///")
                        {
                            lines.push(prev_comment.text().trim().to_string());
                            node = prev_comment;
                            continue;
                        }
                    }
                }
                break;
            }

            lines.reverse();
            tidy.add_arg(name, lines.join("\n"), default);
        }
    }

    tidy
}
