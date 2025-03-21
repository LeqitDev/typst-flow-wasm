use typst::syntax::{LinkedNode, SyntaxNode};

pub fn get_prev<'b>(node: &LinkedNode<'b>) -> Option<LinkedNode<'b>> {
    let parent = node.parent()?;
    if node.index() == 0 {
        return get_prev(parent);
    }
    let index = node.index().checked_sub(1)?;
    let syntax_node = parent.clone().children().nth(index)?;
    Some(syntax_node.clone())
}

pub fn get_prev_kind(node: &LinkedNode<'_>) -> Option<typst::syntax::SyntaxKind> {
    let prev_sibling = get_prev(node)?;
    Some(prev_sibling.kind())
}

pub fn get_args<'b>(node: &LinkedNode<'b>) -> Option<Vec<LinkedNode<'b>>> {
    let closure = if node.kind() == typst::syntax::SyntaxKind::LetBinding {
        node.children()
            .find(|n| n.kind() == typst::syntax::SyntaxKind::Closure)?
    } else if node.kind() == typst::syntax::SyntaxKind::Closure {
        node.clone()
    } else {
        return None;
    };
    let parameters = closure
        .children()
        .find(|n| n.kind() == typst::syntax::SyntaxKind::Params)?;
    let args = parameters
        .children()
        .filter(|n| {
            n.kind() == typst::syntax::SyntaxKind::Ident
                || n.kind() == typst::syntax::SyntaxKind::Named
        })
        .collect();

    Some(args)
}

pub fn is_function(node: &LinkedNode<'_>) -> bool {
    (node.kind() == typst::syntax::SyntaxKind::LetBinding
        && node
            .children()
            .any(|n| n.kind() == typst::syntax::SyntaxKind::Closure))
        || (node.kind() == typst::syntax::SyntaxKind::Ident
            && node.parent().map_or(false, |n| {
                n.children()
                    .any(|n| n.kind() == typst::syntax::SyntaxKind::Closure)
            }))
}

pub fn is_variable(node: &LinkedNode<'_>) -> bool {
    node.kind() == typst::syntax::SyntaxKind::LetBinding
        || (node.kind() == typst::syntax::SyntaxKind::Ident
            && node
                .parent()
                .map_or(false, |n| n.kind() == typst::syntax::SyntaxKind::LetBinding))
}
