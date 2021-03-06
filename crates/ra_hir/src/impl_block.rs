use std::sync::Arc;
use rustc_hash::FxHashMap;

use ra_arena::{Arena, RawId, impl_arena_id};
use ra_syntax::ast::{self, AstNode};
use ra_db::{LocationIntener, Cancelable, SourceRootId};

use crate::{
    DefId, DefLoc, DefKind, SourceItemId, SourceFileItems,
    Function,
    db::HirDatabase,
    type_ref::TypeRef,
    module_tree::ModuleId,
};

use crate::code_model_api::{Module, ModuleSource};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplBlock {
    module_impl_blocks: Arc<ModuleImplBlocks>,
    impl_id: ImplId,
}

impl ImplBlock {
    pub(crate) fn containing(
        module_impl_blocks: Arc<ModuleImplBlocks>,
        def_id: DefId,
    ) -> Option<ImplBlock> {
        let impl_id = *module_impl_blocks.impls_by_def.get(&def_id)?;
        Some(ImplBlock {
            module_impl_blocks,
            impl_id,
        })
    }

    fn impl_data(&self) -> &ImplData {
        &self.module_impl_blocks.impls[self.impl_id]
    }

    pub fn target_trait(&self) -> Option<&TypeRef> {
        self.impl_data().target_trait.as_ref()
    }

    pub fn target_type(&self) -> &TypeRef {
        &self.impl_data().target_type
    }

    pub fn items(&self) -> &[ImplItem] {
        &self.impl_data().items
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplData {
    target_trait: Option<TypeRef>,
    target_type: TypeRef,
    items: Vec<ImplItem>,
}

impl ImplData {
    pub(crate) fn from_ast(
        db: &impl AsRef<LocationIntener<DefLoc, DefId>>,
        file_items: &SourceFileItems,
        module: &Module,
        node: ast::ImplBlock,
    ) -> Self {
        let target_trait = node.target_type().map(TypeRef::from_ast);
        let target_type = TypeRef::from_ast_opt(node.target_type());
        let module_loc = module.def_id.loc(db);
        let items = if let Some(item_list) = node.item_list() {
            item_list
                .impl_items()
                .map(|item_node| {
                    let kind = match item_node {
                        ast::ImplItem::FnDef(..) => DefKind::Function,
                        ast::ImplItem::ConstDef(..) => DefKind::Item,
                        ast::ImplItem::TypeDef(..) => DefKind::Item,
                    };
                    let item_id = file_items.id_of_unchecked(item_node.syntax());
                    let source_item_id = SourceItemId {
                        file_id: module_loc.source_item_id.file_id,
                        item_id: Some(item_id),
                    };
                    let def_loc = DefLoc {
                        kind,
                        source_item_id,
                        ..module_loc
                    };
                    let def_id = def_loc.id(db);
                    match item_node {
                        ast::ImplItem::FnDef(..) => ImplItem::Method(Function::new(def_id)),
                        ast::ImplItem::ConstDef(..) => ImplItem::Const(def_id),
                        ast::ImplItem::TypeDef(..) => ImplItem::Type(def_id),
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        ImplData {
            target_trait,
            target_type,
            items,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImplItem {
    Method(Function),
    // these don't have their own types yet
    Const(DefId),
    Type(DefId),
    // Existential
}

impl ImplItem {
    pub fn def_id(&self) -> DefId {
        match self {
            ImplItem::Method(f) => f.def_id(),
            ImplItem::Const(def_id) => *def_id,
            ImplItem::Type(def_id) => *def_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImplId(pub RawId);
impl_arena_id!(ImplId);

/// Collection of impl blocks is a two-step process: First we collect the blocks
/// per-module; then we build an index of all impl blocks in the crate. This
/// way, we avoid having to do this process for the whole crate whenever someone
/// types in any file; as long as the impl blocks in the file don't change, we
/// don't need to do the second step again.
///
/// (The second step does not yet exist currently.)
#[derive(Debug, PartialEq, Eq)]
pub struct ModuleImplBlocks {
    impls: Arena<ImplId, ImplData>,
    impls_by_def: FxHashMap<DefId, ImplId>,
}

impl ModuleImplBlocks {
    fn new() -> Self {
        ModuleImplBlocks {
            impls: Arena::default(),
            impls_by_def: FxHashMap::default(),
        }
    }

    fn collect(&mut self, db: &impl HirDatabase, module: Module) -> Cancelable<()> {
        let (file_id, module_source) = module.defenition_source(db)?;
        let node = match &module_source {
            ModuleSource::SourceFile(node) => node.borrowed().syntax(),
            ModuleSource::Module(node) => node.borrowed().syntax(),
        };

        let source_file_items = db.file_items(file_id.into());

        for impl_block_ast in node.children().filter_map(ast::ImplBlock::cast) {
            let impl_block = ImplData::from_ast(db, &source_file_items, &module, impl_block_ast);
            let id = self.impls.alloc(impl_block);
            for impl_item in &self.impls[id].items {
                self.impls_by_def.insert(impl_item.def_id(), id);
            }
        }

        Ok(())
    }
}

pub(crate) fn impls_in_module(
    db: &impl HirDatabase,
    source_root_id: SourceRootId,
    module_id: ModuleId,
) -> Cancelable<Arc<ModuleImplBlocks>> {
    let mut result = ModuleImplBlocks::new();
    let module = Module::from_module_id(db, source_root_id, module_id)?;
    result.collect(db, module)?;
    Ok(Arc::new(result))
}
