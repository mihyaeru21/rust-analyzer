use std::ops::Index;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use ra_arena::{Arena, RawId, impl_arena_id, map::ArenaMap};
use ra_db::{LocalSyntaxPtr, Cancelable};
use ra_syntax::ast::{self, AstNode, LoopBodyOwner, ArgListOwner, NameOwner};

use crate::{Path, type_ref::{Mutability, TypeRef}, Name, HirDatabase, DefId, Def, name::AsName};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExprId(RawId);
impl_arena_id!(ExprId);

/// The body of an item (function, const etc.).
#[derive(Debug, Eq, PartialEq)]
pub struct Body {
    exprs: Arena<ExprId, Expr>,
    pats: Arena<PatId, Pat>,
    /// The patterns for the function's arguments. While the argument types are
    /// part of the function signature, the patterns are not (they don't change
    /// the external type of the function).
    ///
    /// If this `Body` is for the body of a constant, this will just be
    /// empty.
    args: Vec<PatId>,
    /// The `ExprId` of the actual body expression.
    body_expr: ExprId,
}

/// An item body together with the mapping from syntax nodes to HIR expression
/// IDs. This is needed to go from e.g. a position in a file to the HIR
/// expression containing it; but for type inference etc., we want to operate on
/// a structure that is agnostic to the actual positions of expressions in the
/// file, so that we don't recompute the type inference whenever some whitespace
/// is typed.
#[derive(Debug, Eq, PartialEq)]
pub struct BodySyntaxMapping {
    body: Arc<Body>,
    expr_syntax_mapping: FxHashMap<LocalSyntaxPtr, ExprId>,
    expr_syntax_mapping_back: ArenaMap<ExprId, LocalSyntaxPtr>,
    pat_syntax_mapping: FxHashMap<LocalSyntaxPtr, PatId>,
    pat_syntax_mapping_back: ArenaMap<PatId, LocalSyntaxPtr>,
}

impl Body {
    pub fn args(&self) -> &[PatId] {
        &self.args
    }

    pub fn body_expr(&self) -> ExprId {
        self.body_expr
    }
}

impl Index<ExprId> for Body {
    type Output = Expr;

    fn index(&self, expr: ExprId) -> &Expr {
        &self.exprs[expr]
    }
}

impl Index<PatId> for Body {
    type Output = Pat;

    fn index(&self, pat: PatId) -> &Pat {
        &self.pats[pat]
    }
}

impl BodySyntaxMapping {
    pub fn expr_syntax(&self, expr: ExprId) -> Option<LocalSyntaxPtr> {
        self.expr_syntax_mapping_back.get(expr).cloned()
    }
    pub fn syntax_expr(&self, ptr: LocalSyntaxPtr) -> Option<ExprId> {
        self.expr_syntax_mapping.get(&ptr).cloned()
    }
    pub fn node_expr(&self, node: ast::Expr) -> Option<ExprId> {
        self.expr_syntax_mapping
            .get(&LocalSyntaxPtr::new(node.syntax()))
            .cloned()
    }
    pub fn pat_syntax(&self, pat: PatId) -> Option<LocalSyntaxPtr> {
        self.pat_syntax_mapping_back.get(pat).cloned()
    }
    pub fn syntax_pat(&self, ptr: LocalSyntaxPtr) -> Option<PatId> {
        self.pat_syntax_mapping.get(&ptr).cloned()
    }
    pub fn node_pat(&self, node: ast::Pat) -> Option<PatId> {
        self.pat_syntax_mapping
            .get(&LocalSyntaxPtr::new(node.syntax()))
            .cloned()
    }

    pub fn body(&self) -> &Arc<Body> {
        &self.body
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Expr {
    /// This is produced if syntax tree does not have a required expression piece.
    Missing,
    Path(Path),
    If {
        condition: ExprId,
        then_branch: ExprId,
        else_branch: Option<ExprId>,
    },
    Block {
        statements: Vec<Statement>,
        tail: Option<ExprId>,
    },
    Loop {
        body: ExprId,
    },
    While {
        condition: ExprId,
        body: ExprId,
    },
    For {
        iterable: ExprId,
        pat: PatId,
        body: ExprId,
    },
    Call {
        callee: ExprId,
        args: Vec<ExprId>,
    },
    MethodCall {
        receiver: ExprId,
        method_name: Name,
        args: Vec<ExprId>,
    },
    Match {
        expr: ExprId,
        arms: Vec<MatchArm>,
    },
    Continue,
    Break {
        expr: Option<ExprId>,
    },
    Return {
        expr: Option<ExprId>,
    },
    StructLit {
        path: Option<Path>,
        fields: Vec<StructLitField>,
        spread: Option<ExprId>,
    },
    Field {
        expr: ExprId,
        name: Name,
    },
    Try {
        expr: ExprId,
    },
    Cast {
        expr: ExprId,
        type_ref: TypeRef,
    },
    Ref {
        expr: ExprId,
        mutability: Mutability,
    },
    UnaryOp {
        expr: ExprId,
        op: Option<UnaryOp>,
    },
    BinaryOp {
        lhs: ExprId,
        rhs: ExprId,
        op: Option<BinaryOp>,
    },
    Lambda {
        args: Vec<PatId>,
        arg_types: Vec<Option<TypeRef>>,
        body: ExprId,
    },
}

pub use ra_syntax::ast::PrefixOp as UnaryOp;
pub use ra_syntax::ast::BinOp as BinaryOp;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MatchArm {
    pub pats: Vec<PatId>,
    // guard: Option<ExprId>, // TODO
    pub expr: ExprId,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StructLitField {
    pub name: Name,
    pub expr: ExprId,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Statement {
    Let {
        pat: PatId,
        type_ref: Option<TypeRef>,
        initializer: Option<ExprId>,
    },
    Expr(ExprId),
}

impl Expr {
    pub fn walk_child_exprs(&self, mut f: impl FnMut(ExprId)) {
        match self {
            Expr::Missing => {}
            Expr::Path(_) => {}
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                f(*condition);
                f(*then_branch);
                if let Some(else_branch) = else_branch {
                    f(*else_branch);
                }
            }
            Expr::Block { statements, tail } => {
                for stmt in statements {
                    match stmt {
                        Statement::Let { initializer, .. } => {
                            if let Some(expr) = initializer {
                                f(*expr);
                            }
                        }
                        Statement::Expr(e) => f(*e),
                    }
                }
                if let Some(expr) = tail {
                    f(*expr);
                }
            }
            Expr::Loop { body } => f(*body),
            Expr::While { condition, body } => {
                f(*condition);
                f(*body);
            }
            Expr::For { iterable, body, .. } => {
                f(*iterable);
                f(*body);
            }
            Expr::Call { callee, args } => {
                f(*callee);
                for arg in args {
                    f(*arg);
                }
            }
            Expr::MethodCall { receiver, args, .. } => {
                f(*receiver);
                for arg in args {
                    f(*arg);
                }
            }
            Expr::Match { expr, arms } => {
                f(*expr);
                for arm in arms {
                    f(arm.expr);
                }
            }
            Expr::Continue => {}
            Expr::Break { expr } | Expr::Return { expr } => {
                if let Some(expr) = expr {
                    f(*expr);
                }
            }
            Expr::StructLit { fields, spread, .. } => {
                for field in fields {
                    f(field.expr);
                }
                if let Some(expr) = spread {
                    f(*expr);
                }
            }
            Expr::Lambda { body, .. } => {
                f(*body);
            }
            Expr::BinaryOp { lhs, rhs, .. } => {
                f(*lhs);
                f(*rhs);
            }
            Expr::Field { expr, .. }
            | Expr::Try { expr }
            | Expr::Cast { expr, .. }
            | Expr::Ref { expr, .. }
            | Expr::UnaryOp { expr, .. } => {
                f(*expr);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PatId(RawId);
impl_arena_id!(PatId);

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Pat {
    Missing,
    Bind {
        name: Name,
    },
    TupleStruct {
        path: Option<Path>,
        args: Vec<PatId>,
    },
}

impl Pat {
    pub fn walk_child_pats(&self, f: impl FnMut(PatId)) {
        match self {
            Pat::Missing | Pat::Bind { .. } => {}
            Pat::TupleStruct { args, .. } => {
                args.iter().map(|pat| *pat).for_each(f);
            }
        }
    }
}

// Queries

pub(crate) fn body_hir(db: &impl HirDatabase, def_id: DefId) -> Cancelable<Arc<Body>> {
    Ok(Arc::clone(&body_syntax_mapping(db, def_id)?.body))
}

struct ExprCollector {
    exprs: Arena<ExprId, Expr>,
    pats: Arena<PatId, Pat>,
    expr_syntax_mapping: FxHashMap<LocalSyntaxPtr, ExprId>,
    expr_syntax_mapping_back: ArenaMap<ExprId, LocalSyntaxPtr>,
    pat_syntax_mapping: FxHashMap<LocalSyntaxPtr, PatId>,
    pat_syntax_mapping_back: ArenaMap<PatId, LocalSyntaxPtr>,
}

impl ExprCollector {
    fn new() -> Self {
        ExprCollector {
            exprs: Arena::default(),
            pats: Arena::default(),
            expr_syntax_mapping: FxHashMap::default(),
            expr_syntax_mapping_back: ArenaMap::default(),
            pat_syntax_mapping: FxHashMap::default(),
            pat_syntax_mapping_back: ArenaMap::default(),
        }
    }

    fn alloc_expr(&mut self, expr: Expr, syntax_ptr: LocalSyntaxPtr) -> ExprId {
        let id = self.exprs.alloc(expr);
        self.expr_syntax_mapping.insert(syntax_ptr, id);
        self.expr_syntax_mapping_back.insert(id, syntax_ptr);
        id
    }

    fn alloc_pat(&mut self, pat: Pat, syntax_ptr: LocalSyntaxPtr) -> PatId {
        let id = self.pats.alloc(pat);
        self.pat_syntax_mapping.insert(syntax_ptr, id);
        self.pat_syntax_mapping_back.insert(id, syntax_ptr);
        id
    }

    fn empty_block(&mut self) -> ExprId {
        let block = Expr::Block {
            statements: Vec::new(),
            tail: None,
        };
        self.exprs.alloc(block)
    }

    fn collect_expr(&mut self, expr: ast::Expr) -> ExprId {
        let syntax_ptr = LocalSyntaxPtr::new(expr.syntax());
        match expr {
            ast::Expr::IfExpr(e) => {
                if let Some(pat) = e.condition().and_then(|c| c.pat()) {
                    // if let -- desugar to match
                    let pat = self.collect_pat(pat);
                    let match_expr =
                        self.collect_expr_opt(e.condition().expect("checked above").expr());
                    let then_branch = self.collect_block_opt(e.then_branch());
                    let else_branch = e
                        .else_branch()
                        .map(|e| self.collect_block(e))
                        .unwrap_or_else(|| self.empty_block());
                    let placeholder_pat = self.pats.alloc(Pat::Missing);
                    let arms = vec![
                        MatchArm {
                            pats: vec![pat],
                            expr: then_branch,
                        },
                        MatchArm {
                            pats: vec![placeholder_pat],
                            expr: else_branch,
                        },
                    ];
                    self.alloc_expr(
                        Expr::Match {
                            expr: match_expr,
                            arms,
                        },
                        syntax_ptr,
                    )
                } else {
                    let condition = self.collect_expr_opt(e.condition().and_then(|c| c.expr()));
                    let then_branch = self.collect_block_opt(e.then_branch());
                    let else_branch = e.else_branch().map(|e| self.collect_block(e));
                    self.alloc_expr(
                        Expr::If {
                            condition,
                            then_branch,
                            else_branch,
                        },
                        syntax_ptr,
                    )
                }
            }
            ast::Expr::BlockExpr(e) => self.collect_block_opt(e.block()),
            ast::Expr::LoopExpr(e) => {
                let body = self.collect_block_opt(e.loop_body());
                self.alloc_expr(Expr::Loop { body }, syntax_ptr)
            }
            ast::Expr::WhileExpr(e) => {
                let condition = if let Some(condition) = e.condition() {
                    if condition.pat().is_none() {
                        self.collect_expr_opt(condition.expr())
                    } else {
                        // TODO handle while let
                        return self.alloc_expr(Expr::Missing, syntax_ptr);
                    }
                } else {
                    self.exprs.alloc(Expr::Missing)
                };
                let body = self.collect_block_opt(e.loop_body());
                self.alloc_expr(Expr::While { condition, body }, syntax_ptr)
            }
            ast::Expr::ForExpr(e) => {
                let iterable = self.collect_expr_opt(e.iterable());
                let pat = self.collect_pat_opt(e.pat());
                let body = self.collect_block_opt(e.loop_body());
                self.alloc_expr(
                    Expr::For {
                        iterable,
                        pat,
                        body,
                    },
                    syntax_ptr,
                )
            }
            ast::Expr::CallExpr(e) => {
                let callee = self.collect_expr_opt(e.expr());
                let args = if let Some(arg_list) = e.arg_list() {
                    arg_list.args().map(|e| self.collect_expr(e)).collect()
                } else {
                    Vec::new()
                };
                self.alloc_expr(Expr::Call { callee, args }, syntax_ptr)
            }
            ast::Expr::MethodCallExpr(e) => {
                let receiver = self.collect_expr_opt(e.expr());
                let args = if let Some(arg_list) = e.arg_list() {
                    arg_list.args().map(|e| self.collect_expr(e)).collect()
                } else {
                    Vec::new()
                };
                let method_name = e
                    .name_ref()
                    .map(|nr| nr.as_name())
                    .unwrap_or_else(Name::missing);
                self.alloc_expr(
                    Expr::MethodCall {
                        receiver,
                        method_name,
                        args,
                    },
                    syntax_ptr,
                )
            }
            ast::Expr::MatchExpr(e) => {
                let expr = self.collect_expr_opt(e.expr());
                let arms = if let Some(match_arm_list) = e.match_arm_list() {
                    match_arm_list
                        .arms()
                        .map(|arm| MatchArm {
                            pats: arm.pats().map(|p| self.collect_pat(p)).collect(),
                            expr: self.collect_expr_opt(arm.expr()),
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                self.alloc_expr(Expr::Match { expr, arms }, syntax_ptr)
            }
            ast::Expr::PathExpr(e) => {
                let path = e
                    .path()
                    .and_then(Path::from_ast)
                    .map(Expr::Path)
                    .unwrap_or(Expr::Missing);
                self.alloc_expr(path, syntax_ptr)
            }
            ast::Expr::ContinueExpr(_e) => {
                // TODO: labels
                self.alloc_expr(Expr::Continue, syntax_ptr)
            }
            ast::Expr::BreakExpr(e) => {
                let expr = e.expr().map(|e| self.collect_expr(e));
                self.alloc_expr(Expr::Break { expr }, syntax_ptr)
            }
            ast::Expr::ParenExpr(e) => {
                let inner = self.collect_expr_opt(e.expr());
                // make the paren expr point to the inner expression as well
                self.expr_syntax_mapping.insert(syntax_ptr, inner);
                inner
            }
            ast::Expr::ReturnExpr(e) => {
                let expr = e.expr().map(|e| self.collect_expr(e));
                self.alloc_expr(Expr::Return { expr }, syntax_ptr)
            }
            ast::Expr::StructLit(e) => {
                let path = e.path().and_then(Path::from_ast);
                let fields = if let Some(nfl) = e.named_field_list() {
                    nfl.fields()
                        .map(|field| StructLitField {
                            name: field
                                .name_ref()
                                .map(|nr| nr.as_name())
                                .unwrap_or_else(Name::missing),
                            expr: if let Some(e) = field.expr() {
                                self.collect_expr(e)
                            } else if let Some(nr) = field.name_ref() {
                                // field shorthand
                                let id = self.exprs.alloc(Expr::Path(Path::from_name_ref(nr)));
                                self.expr_syntax_mapping
                                    .insert(LocalSyntaxPtr::new(nr.syntax()), id);
                                self.expr_syntax_mapping_back
                                    .insert(id, LocalSyntaxPtr::new(nr.syntax()));
                                id
                            } else {
                                self.exprs.alloc(Expr::Missing)
                            },
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                let spread = e.spread().map(|s| self.collect_expr(s));
                self.alloc_expr(
                    Expr::StructLit {
                        path,
                        fields,
                        spread,
                    },
                    syntax_ptr,
                )
            }
            ast::Expr::FieldExpr(e) => {
                let expr = self.collect_expr_opt(e.expr());
                let name = e
                    .name_ref()
                    .map(|nr| nr.as_name())
                    .unwrap_or_else(Name::missing);
                self.alloc_expr(Expr::Field { expr, name }, syntax_ptr)
            }
            ast::Expr::TryExpr(e) => {
                let expr = self.collect_expr_opt(e.expr());
                self.alloc_expr(Expr::Try { expr }, syntax_ptr)
            }
            ast::Expr::CastExpr(e) => {
                let expr = self.collect_expr_opt(e.expr());
                let type_ref = TypeRef::from_ast_opt(e.type_ref());
                self.alloc_expr(Expr::Cast { expr, type_ref }, syntax_ptr)
            }
            ast::Expr::RefExpr(e) => {
                let expr = self.collect_expr_opt(e.expr());
                let mutability = Mutability::from_mutable(e.is_mut());
                self.alloc_expr(Expr::Ref { expr, mutability }, syntax_ptr)
            }
            ast::Expr::PrefixExpr(e) => {
                let expr = self.collect_expr_opt(e.expr());
                let op = e.op();
                self.alloc_expr(Expr::UnaryOp { expr, op }, syntax_ptr)
            }
            ast::Expr::LambdaExpr(e) => {
                let mut args = Vec::new();
                let mut arg_types = Vec::new();
                if let Some(pl) = e.param_list() {
                    for param in pl.params() {
                        let pat = self.collect_pat_opt(param.pat());
                        let type_ref = param.type_ref().map(TypeRef::from_ast);
                        args.push(pat);
                        arg_types.push(type_ref);
                    }
                }
                let body = self.collect_expr_opt(e.body());
                self.alloc_expr(
                    Expr::Lambda {
                        args,
                        arg_types,
                        body,
                    },
                    syntax_ptr,
                )
            }
            ast::Expr::BinExpr(e) => {
                let lhs = self.collect_expr_opt(e.lhs());
                let rhs = self.collect_expr_opt(e.rhs());
                let op = e.op();
                self.alloc_expr(Expr::BinaryOp { lhs, rhs, op }, syntax_ptr)
            }

            // TODO implement HIR for these:
            ast::Expr::Label(_e) => self.alloc_expr(Expr::Missing, syntax_ptr),
            ast::Expr::IndexExpr(_e) => self.alloc_expr(Expr::Missing, syntax_ptr),
            ast::Expr::TupleExpr(_e) => self.alloc_expr(Expr::Missing, syntax_ptr),
            ast::Expr::ArrayExpr(_e) => self.alloc_expr(Expr::Missing, syntax_ptr),
            ast::Expr::RangeExpr(_e) => self.alloc_expr(Expr::Missing, syntax_ptr),
            ast::Expr::Literal(_e) => self.alloc_expr(Expr::Missing, syntax_ptr),
        }
    }

    fn collect_expr_opt(&mut self, expr: Option<ast::Expr>) -> ExprId {
        if let Some(expr) = expr {
            self.collect_expr(expr)
        } else {
            self.exprs.alloc(Expr::Missing)
        }
    }

    fn collect_block(&mut self, block: ast::Block) -> ExprId {
        let statements = block
            .statements()
            .map(|s| match s {
                ast::Stmt::LetStmt(stmt) => {
                    let pat = self.collect_pat_opt(stmt.pat());
                    let type_ref = stmt.type_ref().map(TypeRef::from_ast);
                    let initializer = stmt.initializer().map(|e| self.collect_expr(e));
                    Statement::Let {
                        pat,
                        type_ref,
                        initializer,
                    }
                }
                ast::Stmt::ExprStmt(stmt) => Statement::Expr(self.collect_expr_opt(stmt.expr())),
            })
            .collect();
        let tail = block.expr().map(|e| self.collect_expr(e));
        self.alloc_expr(
            Expr::Block { statements, tail },
            LocalSyntaxPtr::new(block.syntax()),
        )
    }

    fn collect_block_opt(&mut self, block: Option<ast::Block>) -> ExprId {
        if let Some(block) = block {
            self.collect_block(block)
        } else {
            self.exprs.alloc(Expr::Missing)
        }
    }

    fn collect_pat(&mut self, pat: ast::Pat) -> PatId {
        let syntax_ptr = LocalSyntaxPtr::new(pat.syntax());
        match pat {
            ast::Pat::BindPat(bp) => {
                let name = bp
                    .name()
                    .map(|nr| nr.as_name())
                    .unwrap_or_else(Name::missing);
                self.alloc_pat(Pat::Bind { name }, syntax_ptr)
            }
            ast::Pat::TupleStructPat(p) => {
                let path = p.path().and_then(Path::from_ast);
                let args = p.args().map(|p| self.collect_pat(p)).collect();
                self.alloc_pat(Pat::TupleStruct { path, args }, syntax_ptr)
            }
            _ => {
                // TODO
                self.alloc_pat(Pat::Missing, syntax_ptr)
            }
        }
    }

    fn collect_pat_opt(&mut self, pat: Option<ast::Pat>) -> PatId {
        if let Some(pat) = pat {
            self.collect_pat(pat)
        } else {
            self.pats.alloc(Pat::Missing)
        }
    }

    fn into_body_syntax_mapping(self, args: Vec<PatId>, body_expr: ExprId) -> BodySyntaxMapping {
        let body = Body {
            exprs: self.exprs,
            pats: self.pats,
            args,
            body_expr,
        };
        BodySyntaxMapping {
            body: Arc::new(body),
            expr_syntax_mapping: self.expr_syntax_mapping,
            expr_syntax_mapping_back: self.expr_syntax_mapping_back,
            pat_syntax_mapping: self.pat_syntax_mapping,
            pat_syntax_mapping_back: self.pat_syntax_mapping_back,
        }
    }
}

pub(crate) fn collect_fn_body_syntax(node: ast::FnDef) -> BodySyntaxMapping {
    let mut collector = ExprCollector::new();

    let args = if let Some(param_list) = node.param_list() {
        let mut args = Vec::new();

        if let Some(self_param) = param_list.self_param() {
            let self_param = LocalSyntaxPtr::new(
                self_param
                    .self_kw()
                    .expect("self param without self keyword")
                    .syntax(),
            );
            let arg = collector.alloc_pat(
                Pat::Bind {
                    name: Name::self_param(),
                },
                self_param,
            );
            args.push(arg);
        }

        for param in param_list.params() {
            let pat = if let Some(pat) = param.pat() {
                pat
            } else {
                continue;
            };
            args.push(collector.collect_pat(pat));
        }
        args
    } else {
        Vec::new()
    };

    let body = collector.collect_block_opt(node.body());
    collector.into_body_syntax_mapping(args, body)
}

pub(crate) fn body_syntax_mapping(
    db: &impl HirDatabase,
    def_id: DefId,
) -> Cancelable<Arc<BodySyntaxMapping>> {
    let def = def_id.resolve(db)?;

    let body_syntax_mapping = match def {
        Def::Function(f) => {
            let node = f.syntax(db);
            let node = node.borrowed();

            collect_fn_body_syntax(node)
        }
        // TODO: consts, etc.
        _ => panic!("Trying to get body for item type without body"),
    };

    Ok(Arc::new(body_syntax_mapping))
}
