/// This modules specifies the input to rust-analyzer. In some sense, this is
/// **the** most important module, because all other fancy stuff is strictly
/// derived from this input.
///
/// Note that neither this module, nor any other part of the analyzer's core do
/// actual IO. See `vfs` and `project_model` in `ra_lsp_server` crate for how
/// actual IO is done and lowered to input.
use std::sync::Arc;

use relative_path::RelativePathBuf;
use rustc_hash::FxHashMap;
use salsa;

use ra_syntax::SmolStr;
use rustc_hash::FxHashSet;

/// `FileId` is an integer which uniquely identifies a file. File paths are
/// messy and system-dependent, so most of the code should work directly with
/// `FileId`, without inspecting the path. The mapping between `FileId` and path
/// and `SourceRoot` is constant. File rename is represented as a pair of
/// deletion/creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u32);

/// Files are grouped into source roots. A source root is a directory on the
/// file systems which is watched for changes. Typically it corresponds to a
/// Cargo package. Source roots *might* be nested: in this case, file belongs to
/// the nearest enclosing source root. Path to files are always relative to a
/// source root, and analyzer does not know the root path of the source root at
/// all. So, a file from one source root can't refere a file in another source
/// root by path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceRootId(pub u32);

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct SourceRoot {
    pub files: FxHashMap<RelativePathBuf, FileId>,
}

/// `CrateGraph` is a bit of information which turns a set of text files into a
/// number of Rust crates. Each Crate is the `FileId` of it's root module, the
/// set of cfg flags (not yet implemented) and the set of dependencies. Note
/// that, due to cfg's, there might be several crates for a single `FileId`! As
/// in the rust-lang proper, a crate does not have a name. Instead, names are
/// specified on dependency edges. That is, a crate might be known under
/// different names in different dependant crates.
///
/// Note that `CrateGraph` is build-system agnostic: it's a concept of the Rust
/// langauge proper, not a concept of the build system. In practice, we get
/// `CrateGraph` by lowering `cargo metadata` output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CrateGraph {
    arena: FxHashMap<CrateId, CrateData>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CrateId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq)]
struct CrateData {
    file_id: FileId,
    dependencies: Vec<Dependency>,
}

impl CrateData {
    fn new(file_id: FileId) -> CrateData {
        CrateData {
            file_id,
            dependencies: Vec::new(),
        }
    }

    fn add_dep(&mut self, name: SmolStr, crate_id: CrateId) {
        self.dependencies.push(Dependency { name, crate_id })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub crate_id: CrateId,
    pub name: SmolStr,
}

impl Dependency {
    pub fn crate_id(&self) -> CrateId {
        self.crate_id
    }
}

impl CrateGraph {
    pub fn add_crate_root(&mut self, file_id: FileId) -> CrateId {
        let crate_id = CrateId(self.arena.len() as u32);
        let prev = self.arena.insert(crate_id, CrateData::new(file_id));
        assert!(prev.is_none());
        crate_id
    }
    pub fn add_dep(&mut self, from: CrateId, name: SmolStr, to: CrateId) {
        let mut visited = FxHashSet::default();
        if self.dfs_find(from, to, &mut visited) {
            panic!("Cycle dependencies found.")
        }
        self.arena.get_mut(&from).unwrap().add_dep(name, to)
    }
    pub fn is_empty(&self) -> bool {
        self.arena.is_empty()
    }
    pub fn crate_root(&self, crate_id: CrateId) -> FileId {
        self.arena[&crate_id].file_id
    }
    pub fn crate_id_for_crate_root(&self, file_id: FileId) -> Option<CrateId> {
        let (&crate_id, _) = self
            .arena
            .iter()
            .find(|(_crate_id, data)| data.file_id == file_id)?;
        Some(crate_id)
    }
    pub fn dependencies<'a>(
        &'a self,
        crate_id: CrateId,
    ) -> impl Iterator<Item = &'a Dependency> + 'a {
        self.arena[&crate_id].dependencies.iter()
    }
    fn dfs_find(&self, target: CrateId, from: CrateId, visited: &mut FxHashSet<CrateId>) -> bool {
        if !visited.insert(from) {
            return false;
        }

        for dep in self.dependencies(from) {
            let crate_id = dep.crate_id();
            if crate_id == target {
                return true;
            }

            if self.dfs_find(target, crate_id, visited) {
                return true;
            }
        }
        return false;
    }
}

#[cfg(test)]
mod tests {
    use super::{CrateGraph, FxHashMap, FileId, SmolStr};

    #[test]
    #[should_panic]
    fn it_should_painc_because_of_cycle_dependencies() {
        let mut graph = CrateGraph::default();
        let crate1 = graph.add_crate_root(FileId(1u32));
        let crate2 = graph.add_crate_root(FileId(2u32));
        let crate3 = graph.add_crate_root(FileId(3u32));
        graph.add_dep(crate1, SmolStr::new("crate2"), crate2);
        graph.add_dep(crate2, SmolStr::new("crate3"), crate3);
        graph.add_dep(crate3, SmolStr::new("crate1"), crate1);
    }

    #[test]
    fn it_works() {
        let mut graph = CrateGraph {
            arena: FxHashMap::default(),
        };
        let crate1 = graph.add_crate_root(FileId(1u32));
        let crate2 = graph.add_crate_root(FileId(2u32));
        let crate3 = graph.add_crate_root(FileId(3u32));
        graph.add_dep(crate1, SmolStr::new("crate2"), crate2);
        graph.add_dep(crate2, SmolStr::new("crate3"), crate3);
    }
}

salsa::query_group! {
    pub trait FilesDatabase: salsa::Database {
        /// Text of the file.
        fn file_text(file_id: FileId) -> Arc<String> {
            type FileTextQuery;
            storage input;
        }
        /// Path to a file, relative to the root of its source root.
        fn file_relative_path(file_id: FileId) -> RelativePathBuf {
            type FileRelativePathQuery;
            storage input;
        }
        /// Source root of the file.
        fn file_source_root(file_id: FileId) -> SourceRootId {
            type FileSourceRootQuery;
            storage input;
        }
        /// Contents of the source root.
        fn source_root(id: SourceRootId) -> Arc<SourceRoot> {
            type SourceRootQuery;
            storage input;
        }
        /// The set of "local" (that is, from the current workspace) roots.
        /// Files in local roots are assumed to change frequently.
        fn local_roots() -> Arc<Vec<SourceRootId>> {
            type LocalRootsQuery;
            storage input;
        }
        /// The set of roots for crates.io libraries.
        /// Files in libraries are assumed to never change.
        fn library_roots() -> Arc<Vec<SourceRootId>> {
            type LibraryRootsQuery;
            storage input;
        }
        /// The crate graph.
        fn crate_graph() -> Arc<CrateGraph> {
            type CrateGraphQuery;
            storage input;
        }
    }
}
