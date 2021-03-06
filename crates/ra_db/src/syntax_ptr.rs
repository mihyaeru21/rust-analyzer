use ra_syntax::{SourceFileNode, SyntaxKind, SyntaxNode, SyntaxNodeRef, TextRange};

/// A pointer to a syntax node inside a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalSyntaxPtr {
    range: TextRange,
    kind: SyntaxKind,
}

impl LocalSyntaxPtr {
    pub fn new(node: SyntaxNodeRef) -> LocalSyntaxPtr {
        LocalSyntaxPtr {
            range: node.range(),
            kind: node.kind(),
        }
    }

    pub fn resolve(self, file: &SourceFileNode) -> SyntaxNode {
        let mut curr = file.syntax();
        loop {
            if curr.range() == self.range && curr.kind() == self.kind {
                return curr.owned();
            }
            curr = curr
                .children()
                .find(|it| self.range.is_subrange(&it.range()))
                .unwrap_or_else(|| panic!("can't resolve local ptr to SyntaxNode: {:?}", self))
        }
    }

    pub fn range(self) -> TextRange {
        self.range
    }

    pub fn kind(self) -> SyntaxKind {
        self.kind
    }
}

#[test]
fn test_local_syntax_ptr() {
    use ra_syntax::{ast, AstNode};
    let file = SourceFileNode::parse("struct Foo { f: u32, }");
    let field = file
        .syntax()
        .descendants()
        .find_map(ast::NamedFieldDef::cast)
        .unwrap();
    let ptr = LocalSyntaxPtr::new(field.syntax());
    let field_syntax = ptr.resolve(&file);
    assert_eq!(field.syntax(), field_syntax);
}
