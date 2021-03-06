use languageserver_types::{
    self, CreateFile, DocumentChangeOperation, DocumentChanges, InsertTextFormat, Location,
    Position, Range, RenameFile, ResourceOp, SymbolKind, TextDocumentEdit, TextDocumentIdentifier,
    TextDocumentItem, TextDocumentPositionParams, Url, VersionedTextDocumentIdentifier,
    WorkspaceEdit,
};
use ra_analysis::{
    CompletionItem, CompletionItemKind, FileId, FilePosition, FileRange, FileSystemEdit,
    InsertText, NavigationTarget, SourceChange, SourceFileEdit,
};
use ra_editor::{translate_offset_with_edit, LineCol, LineIndex};
use ra_syntax::{SyntaxKind, TextRange, TextUnit};
use ra_text_edit::{AtomTextEdit, TextEdit};

use crate::{req, server_world::ServerWorld, Result};

pub trait Conv {
    type Output;
    fn conv(self) -> Self::Output;
}

pub trait ConvWith {
    type Ctx;
    type Output;
    fn conv_with(self, ctx: &Self::Ctx) -> Self::Output;
}

pub trait TryConvWith {
    type Ctx;
    type Output;
    fn try_conv_with(self, ctx: &Self::Ctx) -> Result<Self::Output>;
}

impl Conv for SyntaxKind {
    type Output = SymbolKind;

    fn conv(self) -> <Self as Conv>::Output {
        match self {
            SyntaxKind::FN_DEF => SymbolKind::Function,
            SyntaxKind::STRUCT_DEF => SymbolKind::Struct,
            SyntaxKind::ENUM_DEF => SymbolKind::Enum,
            SyntaxKind::TRAIT_DEF => SymbolKind::Interface,
            SyntaxKind::MODULE => SymbolKind::Module,
            SyntaxKind::TYPE_DEF => SymbolKind::TypeParameter,
            SyntaxKind::STATIC_DEF => SymbolKind::Constant,
            SyntaxKind::CONST_DEF => SymbolKind::Constant,
            SyntaxKind::IMPL_BLOCK => SymbolKind::Object,
            _ => SymbolKind::Variable,
        }
    }
}

impl Conv for CompletionItemKind {
    type Output = ::languageserver_types::CompletionItemKind;

    fn conv(self) -> <Self as Conv>::Output {
        use languageserver_types::CompletionItemKind::*;
        match self {
            CompletionItemKind::Keyword => Keyword,
            CompletionItemKind::Snippet => Snippet,
            CompletionItemKind::Module => Module,
            CompletionItemKind::Function => Function,
            CompletionItemKind::Struct => Struct,
            CompletionItemKind::Enum => Enum,
            CompletionItemKind::EnumVariant => EnumMember,
            CompletionItemKind::Binding => Variable,
            CompletionItemKind::Field => Field,
        }
    }
}

impl Conv for CompletionItem {
    type Output = ::languageserver_types::CompletionItem;

    fn conv(self) -> <Self as Conv>::Output {
        let mut res = ::languageserver_types::CompletionItem {
            label: self.label().to_string(),
            filter_text: Some(self.lookup().to_string()),
            kind: self.kind().map(|it| it.conv()),
            ..Default::default()
        };
        match self.insert_text() {
            InsertText::PlainText { text } => {
                res.insert_text = Some(text);
                res.insert_text_format = Some(InsertTextFormat::PlainText);
            }
            InsertText::Snippet { text } => {
                res.insert_text = Some(text);
                res.insert_text_format = Some(InsertTextFormat::Snippet);
            }
        }
        res
    }
}

impl ConvWith for Position {
    type Ctx = LineIndex;
    type Output = TextUnit;

    fn conv_with(self, line_index: &LineIndex) -> TextUnit {
        let line_col = LineCol {
            line: self.line as u32,
            col_utf16: self.character as u32,
        };
        line_index.offset(line_col)
    }
}

impl ConvWith for TextUnit {
    type Ctx = LineIndex;
    type Output = Position;

    fn conv_with(self, line_index: &LineIndex) -> Position {
        let line_col = line_index.line_col(self);
        Position::new(u64::from(line_col.line), u64::from(line_col.col_utf16))
    }
}

impl ConvWith for TextRange {
    type Ctx = LineIndex;
    type Output = Range;

    fn conv_with(self, line_index: &LineIndex) -> Range {
        Range::new(
            self.start().conv_with(line_index),
            self.end().conv_with(line_index),
        )
    }
}

impl ConvWith for Range {
    type Ctx = LineIndex;
    type Output = TextRange;

    fn conv_with(self, line_index: &LineIndex) -> TextRange {
        TextRange::from_to(
            self.start.conv_with(line_index),
            self.end.conv_with(line_index),
        )
    }
}

impl ConvWith for TextEdit {
    type Ctx = LineIndex;
    type Output = Vec<languageserver_types::TextEdit>;

    fn conv_with(self, line_index: &LineIndex) -> Vec<languageserver_types::TextEdit> {
        self.as_atoms()
            .into_iter()
            .map_conv_with(line_index)
            .collect()
    }
}

impl<'a> ConvWith for &'a AtomTextEdit {
    type Ctx = LineIndex;
    type Output = languageserver_types::TextEdit;

    fn conv_with(self, line_index: &LineIndex) -> languageserver_types::TextEdit {
        languageserver_types::TextEdit {
            range: self.delete.conv_with(line_index),
            new_text: self.insert.clone(),
        }
    }
}

impl<T: ConvWith> ConvWith for Option<T> {
    type Ctx = <T as ConvWith>::Ctx;
    type Output = Option<<T as ConvWith>::Output>;
    fn conv_with(self, ctx: &Self::Ctx) -> Self::Output {
        self.map(|x| ConvWith::conv_with(x, ctx))
    }
}

impl<'a> TryConvWith for &'a Url {
    type Ctx = ServerWorld;
    type Output = FileId;
    fn try_conv_with(self, world: &ServerWorld) -> Result<FileId> {
        world.uri_to_file_id(self)
    }
}

impl TryConvWith for FileId {
    type Ctx = ServerWorld;
    type Output = Url;
    fn try_conv_with(self, world: &ServerWorld) -> Result<Url> {
        world.file_id_to_uri(self)
    }
}

impl<'a> TryConvWith for &'a TextDocumentItem {
    type Ctx = ServerWorld;
    type Output = FileId;
    fn try_conv_with(self, world: &ServerWorld) -> Result<FileId> {
        self.uri.try_conv_with(world)
    }
}

impl<'a> TryConvWith for &'a VersionedTextDocumentIdentifier {
    type Ctx = ServerWorld;
    type Output = FileId;
    fn try_conv_with(self, world: &ServerWorld) -> Result<FileId> {
        self.uri.try_conv_with(world)
    }
}

impl<'a> TryConvWith for &'a TextDocumentIdentifier {
    type Ctx = ServerWorld;
    type Output = FileId;
    fn try_conv_with(self, world: &ServerWorld) -> Result<FileId> {
        world.uri_to_file_id(&self.uri)
    }
}

impl<'a> TryConvWith for &'a TextDocumentPositionParams {
    type Ctx = ServerWorld;
    type Output = FilePosition;
    fn try_conv_with(self, world: &ServerWorld) -> Result<FilePosition> {
        let file_id = self.text_document.try_conv_with(world)?;
        let line_index = world.analysis().file_line_index(file_id);
        let offset = self.position.conv_with(&line_index);
        Ok(FilePosition { file_id, offset })
    }
}

impl<'a> TryConvWith for (&'a TextDocumentIdentifier, Range) {
    type Ctx = ServerWorld;
    type Output = FileRange;
    fn try_conv_with(self, world: &ServerWorld) -> Result<FileRange> {
        let file_id = self.0.try_conv_with(world)?;
        let line_index = world.analysis().file_line_index(file_id);
        let range = self.1.conv_with(&line_index);
        Ok(FileRange { file_id, range })
    }
}

impl<T: TryConvWith> TryConvWith for Vec<T> {
    type Ctx = <T as TryConvWith>::Ctx;
    type Output = Vec<<T as TryConvWith>::Output>;
    fn try_conv_with(self, ctx: &Self::Ctx) -> Result<Self::Output> {
        let mut res = Vec::with_capacity(self.len());
        for item in self {
            res.push(item.try_conv_with(ctx)?);
        }
        Ok(res)
    }
}

impl TryConvWith for SourceChange {
    type Ctx = ServerWorld;
    type Output = req::SourceChange;
    fn try_conv_with(self, world: &ServerWorld) -> Result<req::SourceChange> {
        let cursor_position = match self.cursor_position {
            None => None,
            Some(pos) => {
                let line_index = world.analysis().file_line_index(pos.file_id);
                let edit = self
                    .source_file_edits
                    .iter()
                    .find(|it| it.file_id == pos.file_id)
                    .map(|it| &it.edit);
                let line_col = match edit {
                    Some(edit) => translate_offset_with_edit(&*line_index, pos.offset, edit),
                    None => line_index.line_col(pos.offset),
                };
                let position =
                    Position::new(u64::from(line_col.line), u64::from(line_col.col_utf16));
                Some(TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier::new(pos.file_id.try_conv_with(world)?),
                    position,
                })
            }
        };
        let mut document_changes: Vec<DocumentChangeOperation> = Vec::new();
        for resource_op in self.file_system_edits.try_conv_with(world)? {
            document_changes.push(DocumentChangeOperation::Op(resource_op));
        }
        for text_document_edit in self.source_file_edits.try_conv_with(world)? {
            document_changes.push(DocumentChangeOperation::Edit(text_document_edit));
        }
        let workspace_edit = WorkspaceEdit {
            changes: None,
            document_changes: Some(DocumentChanges::Operations(document_changes)),
        };
        Ok(req::SourceChange {
            label: self.label,
            workspace_edit,
            cursor_position,
        })
    }
}

impl TryConvWith for SourceFileEdit {
    type Ctx = ServerWorld;
    type Output = TextDocumentEdit;
    fn try_conv_with(self, world: &ServerWorld) -> Result<TextDocumentEdit> {
        let text_document = VersionedTextDocumentIdentifier {
            uri: self.file_id.try_conv_with(world)?,
            version: None,
        };
        let line_index = world.analysis().file_line_index(self.file_id);
        let edits = self
            .edit
            .as_atoms()
            .iter()
            .map_conv_with(&line_index)
            .collect();
        Ok(TextDocumentEdit {
            text_document,
            edits,
        })
    }
}

impl TryConvWith for FileSystemEdit {
    type Ctx = ServerWorld;
    type Output = ResourceOp;
    fn try_conv_with(self, world: &ServerWorld) -> Result<ResourceOp> {
        let res = match self {
            FileSystemEdit::CreateFile { source_root, path } => {
                let uri = world.path_to_uri(source_root, &path)?.to_string();
                ResourceOp::Create(CreateFile { uri, options: None })
            }
            FileSystemEdit::MoveFile {
                src,
                dst_source_root,
                dst_path,
            } => {
                let old_uri = world.file_id_to_uri(src)?.to_string();
                let new_uri = world.path_to_uri(dst_source_root, &dst_path)?.to_string();
                ResourceOp::Rename(RenameFile {
                    old_uri,
                    new_uri,
                    options: None,
                })
            }
        };
        Ok(res)
    }
}

impl TryConvWith for &NavigationTarget {
    type Ctx = ServerWorld;
    type Output = Location;
    fn try_conv_with(self, world: &ServerWorld) -> Result<Location> {
        let line_index = world.analysis().file_line_index(self.file_id());
        to_location(self.file_id(), self.range(), &world, &line_index)
    }
}

pub fn to_location(
    file_id: FileId,
    range: TextRange,
    world: &ServerWorld,
    line_index: &LineIndex,
) -> Result<Location> {
    let url = file_id.try_conv_with(world)?;
    let loc = Location::new(url, range.conv_with(line_index));
    Ok(loc)
}

pub trait MapConvWith<'a>: Sized + 'a {
    type Ctx;
    type Output;

    fn map_conv_with(self, ctx: &'a Self::Ctx) -> ConvWithIter<'a, Self, Self::Ctx> {
        ConvWithIter { iter: self, ctx }
    }
}

impl<'a, I> MapConvWith<'a> for I
where
    I: Iterator + 'a,
    I::Item: ConvWith,
{
    type Ctx = <I::Item as ConvWith>::Ctx;
    type Output = <I::Item as ConvWith>::Output;
}

pub struct ConvWithIter<'a, I, Ctx: 'a> {
    iter: I,
    ctx: &'a Ctx,
}

impl<'a, I, Ctx> Iterator for ConvWithIter<'a, I, Ctx>
where
    I: Iterator,
    I::Item: ConvWith<Ctx = Ctx>,
{
    type Item = <I::Item as ConvWith>::Output;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|item| item.conv_with(self.ctx))
    }
}
